//! The Rune UI: a table of `[nav]` bindings you can walk with the keyboard.
//!
//! Task 11 of the port: the shell, the table, selection and filtering. The keyboard map, the
//! action picker, packs, the monitor and apply arrive on top of this state.

use rune::tok::{self, radius, type_};
use rune::{AppSpec, Cx, El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};

use crate::cli::Paths;
use crate::keyd::config::{Binding, Tap, parse, read_tap, tap_label};
use crate::keyd::data::action_description;
use crate::keyd::rules::{History, filter_binding_indices};
use crate::keyd::system::{CheckOutcome, check};

pub mod edit;
pub mod map;

/// Select the binding for `key`, clearing the filter when it hides the row; an unbound cap
/// opens the picker so a click can add a binding.
pub fn select_key(s: &mut State, key: &str) {
    let visible = s.visible();
    if let Some(position) = visible
        .iter()
        .position(|&index| s.binds[index].key.eq_ignore_ascii_case(key))
    {
        s.selected = position;
        s.status.clear();
        return;
    }
    if s.binds.iter().any(|binding| binding.key.eq_ignore_ascii_case(key)) {
        s.filter.clear();
        s.selected = s
            .visible()
            .iter()
            .position(|&index| s.binds[index].key.eq_ignore_ascii_case(key))
            .unwrap_or(0);
        s.status.clear();
    } else {
        edit::open_picker(s, key);
    }
}

/// Validate and write the config.
pub fn save(s: &mut State) {
    if let Some(error) = &s.load_error {
        s.status = format!("not saving: {error}");
        return;
    }
    match (s.checker)(&s.text) {
        Ok(CheckOutcome::Rejected(message)) => s.status = format!("keyd rejects this: {message}"),
        Err(error) => s.status = format!("could not run keyd check: {error}"),
        Ok(_) => match std::fs::write(&s.paths.conf, &s.text) {
            Ok(()) => {
                s.dirty = false;
                s.status = "Saved.".to_string();
                let _ = std::fs::remove_file(unsaved_path(&s.paths.conf));
                refresh_live(s);
            }
            Err(error) => {
                s.status = format!("cannot write {}: {error}", s.paths.conf.display());
            }
        },
    }
}

/// Everything the window draws and the key handler edits.
pub struct State {
    pub paths: Paths,
    /// The checkout's config, as last read or saved.
    pub text: String,
    pub binds: Vec<Binding>,
    pub tap: Tap,
    /// Index into the *visible* rows.
    pub selected: usize,
    pub filter: String,
    pub filtering: bool,
    pub status: String,
    /// The open overlay, if any.
    pub edit: edit::Edit,
    pub history: History<edit::Snapshot>,
    /// Whether the buffer differs from the file.
    pub dirty: bool,
    /// How a candidate config is validated; tests swap in a stub.
    pub checker: fn(&str) -> std::io::Result<CheckOutcome>,
    /// The live keyd stream, while the panel is open.
    pub monitor_stream: Option<crate::keyd::system::MonitorStream>,
    pub monitor: crate::keyd::monitor::MonitorLog,
    pub monitor_status: String,
    /// Rune's redraw waker, handed over in `setup`; the reader threads call it.
    pub waker: Option<crate::keyd::system::Waker>,
    /// Overrides how the monitor is started; tests and scripted runs set it.
    pub monitor_command: Option<Vec<String>>,
    /// How an apply asks for root; `run` fills it from the environment.
    pub privilege: crate::keyd::privilege::Priv,
    /// Set when the config could not be read: saving and applying are refused.
    pub load_error: Option<String>,
    /// Set by the first `q` with unsaved edits; the second one quits.
    pub quit_armed: bool,
    /// Whether the buffer matches the live config, computed when the panel opens and after a
    /// save or apply — not per frame, which would read `/etc/keyd` on every redraw.
    pub live_state: LiveState,
    /// Seconds since the app started when the capture began; the timeout rides Rune's clock.
    pub capture_started: Option<f64>,
}

/// How the buffer compares with the live config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LiveState {
    #[default]
    Unknown,
    Matches,
    Differs,
    Unreadable,
}

impl LiveState {
    pub fn label(self) -> &'static str {
        match self {
            LiveState::Unknown => "",
            LiveState::Matches => "editor matches the live config",
            LiveState::Differs => "editor differs from the live config",
            LiveState::Unreadable => "live config unreadable",
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            paths: Paths::from_env(),
            text: String::new(),
            binds: Vec::new(),
            tap: Tap { action: None, editable: true, message: String::new() },
            selected: 0,
            filter: String::new(),
            filtering: false,
            status: String::new(),
            edit: edit::Edit::default(),
            history: History::new(50),
            dirty: false,
            checker: check,
            monitor_stream: None,
            monitor: crate::keyd::monitor::MonitorLog::default(),
            monitor_status: String::new(),
            waker: None,
            monitor_command: None,
            privilege: crate::keyd::privilege::Priv::SudoTty,
            load_error: None,
            quit_armed: false,
            live_state: LiveState::Unknown,
            capture_started: None,
        }
    }
}

impl State {
    /// Load the checkout's config; a failure is kept in `load_error` so the window can say so
    /// and refuse to save or apply over it.
    pub fn load(paths: Paths) -> Self {
        let mut state = Self { paths, ..Self::default() };
        match std::fs::read_to_string(&state.paths.conf) {
            Ok(text) => {
                state.apply_text(text);
                let parked = unsaved_path(&state.paths.conf);
                if parked.is_file() {
                    state.status = format!(
                        "unsaved edits from the last session are in {}",
                        parked.display()
                    );
                }
            }
            Err(error) => {
                state.load_error = Some(format!("cannot read {}: {error}", state.paths.conf.display()));
                state.status = state.load_error.clone().unwrap_or_default();
            }
        }
        state
    }

    /// Read the config again, reporting a failure in the status line.
    pub fn reload(&mut self) {
        match std::fs::read_to_string(&self.paths.conf) {
            Ok(text) => self.apply_text(text),
            Err(error) => {
                self.status = format!("cannot read {}: {error}", self.paths.conf.display());
            }
        }
    }

    fn apply_text(&mut self, text: String) {
        self.binds = parse(&text);
        self.tap = read_tap(&text);
        self.text = text;
        self.status.clear();
    }

    /// Indexes of the bindings the filter lets through.
    pub fn visible(&self) -> Vec<usize> {
        filter_binding_indices(&self.binds, &self.filter)
    }

    /// The binding under the cursor, if any.
    pub fn current(&self) -> Option<&Binding> {
        let visible = self.visible();
        visible.get(self.selected).map(|&index| &self.binds[index])
    }

    fn move_by(&mut self, delta: isize) {
        let count = self.visible().len();
        if count == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, count as isize - 1) as usize;
    }

    /// The config path, shortened for the header.
    pub fn conf_label(&self) -> String {
        self.paths.conf.display().to_string()
    }
}

fn keycap(t: &tok::Theme, label: &str, selected: bool) -> El {
    let (fill, ink) = if selected {
        (t.bg, tok::accent(0, true))
    } else {
        (t.control_off, t.text)
    };
    el().w(72.0)
        .h(22.0)
        .radius(6.0)
        .fill(fill)
        .center()
        .child(text(label.to_string()).font(type_::LABEL).color(ink))
}

fn binding_row(t: &tok::Theme, binding: &Binding, index: usize, selected: bool) -> El {
    let (fill, ink, muted) = if selected {
        (tok::accent(0, true), t.bg, t.bg)
    } else {
        (t.card, t.text, t.text_secondary)
    };
    let label = crate::keyd::data::key_label(&binding.key);
    let description = action_description(&binding.action).unwrap_or("");
    row()
        .key(format!("row-{index}"))
        .h(34.0)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .radius(radius::SM)
        .fill(fill)
        .child(keycap(t, &label, selected))
        .child(text(binding.action.clone()).font(type_::BODY).color(ink))
        .child(text(description.to_string()).font(type_::LABEL).grow(1.0).color(muted))
}

/// One hint: the key as a small cap, then what it does.
fn hint(t: &tok::Theme, key: &str, label: &str, primary: bool) -> El {
    let key_ink = if primary { t.text } else { t.text_secondary };
    row()
        .gap(5.0)
        .center_y()
        .child(
            el().h(17.0)
                .px(5.0)
                .radius(4.0)
                .fill(t.control_off)
                .center()
                .child(text(key.to_string()).font(type_::SECTION).color(key_ink)),
        )
        .child(text(label.to_string()).font(type_::SECTION).color(t.text_secondary))
}

/// The hints under the table: the everyday keys first, the rest dimmer.
fn hints(t: &tok::Theme) -> El {
    let primary = row()
        .key("hints-primary")
        .gap(14.0)
        .center_y()
        .child(hint(t, "↑↓", "move", true))
        .child(hint(t, "/", "filter", true))
        .child(hint(t, "Enter", "edit", true))
        .child(hint(t, "a", "capture", true))
        .child(hint(t, "s", "save", true))
        .child(hint(t, "A", "apply", true))
        .child(hint(t, "q", "quit", true));
    let secondary = row()
        .key("hints-secondary")
        .gap(14.0)
        .center_y()
        .child(hint(t, "d", "delete", false))
        .child(hint(t, "t", "tap", false))
        .child(hint(t, "p", "packs", false))
        .child(hint(t, "b", "backups", false))
        .child(hint(t, "l", "monitor", false))
        .child(hint(t, "u/U", "undo/redo", false));
    col().w_full().gap(6.0).child(primary).child(secondary)
}

/// The window.
pub fn view(s: &State, _: &mut Cx<State>) -> El {
    let t = tok::theme(true);
    let visible = s.visible();

    let mut list = col().key("bindings").gap(4.0);
    if visible.is_empty() {
        list = list.child(
            text(if s.filter.is_empty() {
                "No bindings in [nav].".to_string()
            } else {
                format!("Nothing matches {:?}.", s.filter)
            })
            .font(type_::BODY)
            .color(t.text_secondary),
        );
    }
    for (row_index, &index) in visible.iter().enumerate() {
        list = list.child(binding_row(&t, &s.binds[index], index, row_index == s.selected));
    }

    let header = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(text("Caps layer").font(type_::TITLE).color(t.text))
        .child(
            text(format!("tap: {}", tap_label(&s.tap)))
                .font(type_::LABEL)
                .grow(1.0)
                .color(t.text_secondary),
        )
        .child(text(s.conf_label()).font(type_::SECTION).color(t.text_secondary));

    let footer = if s.filtering {
        text(format!("/{}", s.filter)).font(type_::BODY).color(t.text)
    } else if s.filter.is_empty() {
        hints(&t)
    } else {
        row()
            .gap(14.0)
            .center_y()
            .child(text(format!("filter {:?}", s.filter)).font(type_::SECTION).color(t.text))
            .child(hint(&t, "Esc", "clear", true))
            .child(hint(&t, "Enter", "edit", true))
            .child(hint(&t, "s", "save", true))
    };

    let body = if s.edit.is_open() {
        edit::panel(s, &t)
    } else {
        list.scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0)
    };

    col()
        .w_full()
        .h_full()
        .padding(20.0)
        .gap(12.0)
        .fill(t.bg)
        .child(header)
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(map::map(s, &t))
        .child(el().h(1.0).w_full().fill(t.separator))
        .child(body)
        .child(footer)
        .child(text(s.status.clone()).font(type_::SECTION).color(t.text_secondary))
}

/// Keys: arrows and j/k move, `/` filters, Enter edits, d deletes, s saves, u/U undo/redo, q quits.
pub fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    // Any key but q means the user is still working: the next q must warn again.
    if !matches!(&ev.key, Key::Text(typed) if typed == "q") {
        s.quit_armed = false;
    }
    if s.edit.is_open() {
        return edit::on_key(s, ev);
    }
    if s.filtering {
        match &ev.key {
            Key::Text(typed) => {
                s.filter.push_str(typed);
                s.selected = 0;
                return true;
            }
            Key::Backspace => {
                s.filter.pop();
                s.selected = 0;
                return true;
            }
            Key::Enter => {
                s.filtering = false;
                return true;
            }
            Key::Escape => {
                s.filtering = false;
                s.filter.clear();
                s.selected = 0;
                return true;
            }
            _ => return false,
        }
    }
    match &ev.key {
        Key::Up => {
            s.move_by(-1);
            true
        }
        Key::Down => {
            s.move_by(1);
            true
        }
        Key::Text(typed) if typed == "k" => {
            s.move_by(-1);
            true
        }
        Key::Text(typed) if typed == "j" => {
            s.move_by(1);
            true
        }
        Key::Text(typed) if typed == "/" => {
            s.filtering = true;
            s.filter.clear();
            s.selected = 0;
            true
        }
        Key::Enter => {
            if let Some(key) = s.current().map(|binding| binding.key.clone()) {
                edit::open_picker(s, &key);
            }
            true
        }
        Key::Text(typed) if typed == "e" => {
            if let Some(key) = s.current().map(|binding| binding.key.clone()) {
                edit::open_picker(s, &key);
            }
            true
        }
        Key::Text(typed) if typed == "d" => {
            if let Some(key) = s.current().map(|binding| binding.key.clone()) {
                edit::remove_binding(s, &key);
            }
            true
        }
        Key::Text(typed) if typed == "t" => {
            edit::open_tap(s);
            true
        }
        Key::Text(typed) if typed == "p" => {
            edit::open_packs(s);
            true
        }
        Key::Text(typed) if typed == "b" => {
            edit::open_backups(s);
            true
        }
        Key::Text(typed) if typed == "l" => {
            open_monitor(s);
            true
        }
        Key::Text(typed) if typed == "A" => {
            edit::open_apply(s);
            true
        }
        Key::Text(typed) if typed == "s" => {
            save(s);
            true
        }
        Key::Text(typed) if typed == "u" => {
            edit::undo(s);
            true
        }
        Key::Text(typed) if typed == "U" => {
            edit::redo(s);
            true
        }
        Key::Escape => {
            s.filter.clear();
            s.selected = 0;
            true
        }
        Key::Text(typed) if typed == "a" => {
            let now = tx.clock();
            open_capture(s, now);
            // An idle window draws no frames, so the timeout needs a timer, not a poll.
            tx.wake_at(now + 10.0);
            true
        }
        Key::Text(typed) if typed == "q" => {
            if s.dirty && !s.quit_armed {
                s.quit_armed = true;
                s.status = "Unsaved changes — press q again to quit, or s to save.".to_string();
            } else {
                tx.quit();
            }
            true
        }
        _ => false,
    }
}

/// Compare the buffer with the live config, once.
pub fn refresh_live(s: &mut State) {
    let (live, unreadable) = crate::keyd::system::read_live_config_in(&s.paths.etc, &s.text);
    s.live_state = if unreadable {
        LiveState::Unreadable
    } else if live == s.text {
        LiveState::Matches
    } else {
        LiveState::Differs
    };
}

/// Start the live monitor and open its panel.
///
/// From a `.desktop` launch there is no terminal and no cached sudo credentials, so this goes
/// through the polkit helper. `KEYMAP_MONITOR_CMD` stays the test and verify override.
pub fn open_monitor(s: &mut State) {
    refresh_live(s);
    match start_stream(s) {
        Ok(stream) => {
            s.monitor_stream = Some(stream);
            s.monitor_status = "monitoring".to_string();
            s.edit.mode = Some(edit::Mode::Monitor);
        }
        Err(error) => s.status = format!("cannot start keyd monitor: {error}"),
    }
}

/// Wait for one key press and open the picker for it.
pub fn open_capture(s: &mut State, now: f64) {
    // One stream at a time: the `l` panel and a capture must not share one.
    s.monitor_stream = None;
    match start_stream(s) {
        Ok(stream) => {
            s.monitor_stream = Some(stream);
            s.monitor_status = "capturing".to_string();
            s.capture_started = Some(now);
            s.edit = edit::Edit {
                mode: Some(edit::Mode::Capture),
                ..edit::Edit::default()
            };
        }
        Err(error) => s.status = format!("cannot start keyd monitor: {error}"),
    }
}

fn start_stream(s: &State) -> std::io::Result<crate::keyd::system::MonitorStream> {
    let waker = s.waker.clone();
    if let Some(command) = s.monitor_command.clone() {
        crate::keyd::system::MonitorStream::spawn_with(&command, waker)
    } else if std::env::var_os("KEYMAP_MONITOR_CMD").is_some() {
        let (argv, _) = crate::keyd::system::monitor_command();
        crate::keyd::system::MonitorStream::spawn_with(&argv, waker)
    } else {
        crate::keyd::privilege::spawn_monitor(&crate::keyd::privilege::detect(), waker)
    }
}

/// Drain the live stream into the log; true when the window should redraw.
pub fn poll_monitor(s: &mut State) -> bool {
    let (lines, status) = match s.monitor_stream.as_mut() {
        Some(stream) => stream.poll(),
        None => return false,
    };
    let mut changed = !lines.is_empty();
    let capturing = s.edit.mode == Some(edit::Mode::Capture);
    let mut captured: Option<String> = None;
    for line in &lines {
        if capturing && captured.is_none() {
            // Same rule as the TUI: the first key-down that is not Caps Lock. Modifiers are
            // capturable, because `leftshift` and `rightalt` are valid `[nav]` keys.
            captured = crate::keyd::system::parse_monitor_line(line);
        }
        s.monitor.record(line, &s.binds);
    }
    if let Some(key) = captured {
        // One key is all a capture needs: stop the stream and pick an action for it.
        s.monitor_stream = None;
        s.capture_started = None;
        s.monitor_status = format!("captured {key}");
        edit::open_picker(s, &key);
        return true;
    }
    if let Some(status) = status {
        // The stream is over; dropping it stops the redraw requests.
        s.monitor_status = format!("exited: {status}");
        s.monitor_stream = None;
        changed = true;
    }
    changed
}

/// The per-frame hook Rune calls: drains the monitor and asks for a redraw when it changed.
pub fn poll_hook(s: &mut State, seconds: f64, _tx: &mut Tx) -> bool {
    if s.edit.mode == Some(edit::Mode::Capture)
        && s.capture_started.is_some_and(|start| seconds - start > 10.0)
    {
        s.monitor_stream = None;
        s.capture_started = None;
        s.monitor_status = "capture timed out".to_string();
        s.status = "Capture timed out; press a again, or Enter to pick an action.".to_string();
        s.edit.close();
        return true;
    }
    poll_monitor(s)
}

/// Called once before the first frame: keep Rune's waker so the monitor can ask for a redraw.
fn setup(s: &mut State, waker: rune::Waker) {
    s.waker = Some(waker);
}

/// Where unsaved edits are parked when the window closes.
pub fn unsaved_path(conf: &std::path::Path) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}.unsaved", conf.display()))
}

/// The close button cannot be vetoed, so the buffer is parked beside the config instead.
fn on_close(s: &mut State, _tx: &mut Tx) {
    if s.dirty {
        let _ = std::fs::write(unsaved_path(&s.paths.conf), &s.text);
    }
}

/// Open the window.
pub fn run() {
    let mut state = State::load(Paths::from_env());
    state.privilege = crate::keyd::privilege::detect();
    rune::App::new(AppSpec::<State> {
        scenes: &[
            ("picker", |s: &mut State| edit::open_picker(s, "left")),
            ("macro", |s: &mut State| {
                edit::open_picker(s, "left");
                s.edit.mode = Some(edit::Mode::Macro);
                s.edit.draft.steps = vec![
                    crate::keyd::macros::MacroStep::Text("hi".into()),
                    crate::keyd::macros::MacroStep::Delay(50),
                    crate::keyd::macros::MacroStep::Key("enter".into()),
                ];
            }),
            ("monitor", |s: &mut State| {
                let binds = s.binds.clone();
                for line in [
                    "kbd\tDaKai\tleftcontrol down",
                    "kbd\tDaKai\tleft down",
                    "kbd\tDaKai\tleft up",
                    "kbd\tDaKai\tleftcontrol up",
                    "kbd\tDaKai\tcapslock down",
                    "kbd\tDaKai\tcapslock up",
                ] {
                    s.monitor.record(line, &binds);
                }
                s.monitor_status = "monitoring".to_string();
                s.edit.mode = Some(edit::Mode::Monitor);
            }),
            ("apply", |s: &mut State| edit::open_apply(s)),
        ],
        on_key: Some(on_key),
        poll: Some(poll_hook),
        setup: Some(setup),
        ..AppSpec::new("caps-gui", (980.0, 760.0), State::default, view)
    })
    .init(move || state)
    .on_close(on_close)
    .run();
}
