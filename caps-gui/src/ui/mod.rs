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
    match (s.checker)(&s.text) {
        Ok(CheckOutcome::Rejected(message)) => s.status = format!("keyd rejects this: {message}"),
        Err(error) => s.status = format!("could not run keyd check: {error}"),
        Ok(_) => match std::fs::write(&s.paths.conf, &s.text) {
            Ok(()) => {
                s.dirty = false;
                s.status = "Saved.".to_string();
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
        }
    }
}

impl State {
    /// Load the checkout's config.
    pub fn load(paths: Paths) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(&paths.conf)?;
        let mut state = Self { paths, ..Self::default() };
        state.apply_text(text);
        Ok(state)
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
        format!("/{}", s.filter)
    } else if s.filter.is_empty() {
        "↑↓ move · / filter · Enter edit · d delete · s save · u/U undo/redo · q quit".to_string()
    } else {
        format!("filter {:?} · Esc clears · Enter edit · s save", s.filter)
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
        .child(text(footer).font(type_::SECTION).color(t.text_secondary))
        .child(text(s.status.clone()).font(type_::SECTION).color(t.text_secondary))
}

/// Keys: arrows and j/k move, `/` filters, Enter edits, d deletes, s saves, u/U undo/redo, q quits.
pub fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
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
        Key::Text(typed) if typed == "q" => {
            tx.quit();
            true
        }
        _ => false,
    }
}

/// Open the window.
pub fn run() {
    let state = State::load(Paths::from_env()).unwrap_or_else(|error| {
        let mut state = State::default();
        state.status = format!("cannot read {}: {error}", state.paths.conf.display());
        state
    });
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
        ],
        on_key: Some(on_key),
        ..AppSpec::new("caps-gui", (980.0, 760.0), State::default, view)
    })
    .init(move || state)
    .run();
}
