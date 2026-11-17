//! Non-interactive command line for the keyd Caps Lock editor.
//!
//! Ported from `keymapper/cli.py`. Paths come from `Paths`, which `main` builds from
//! `CAPS_CONF` / `CAPS_INSTALL` / `KEYMAP_ETC`; tests pass their own.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::keyd::config::{diff_lines, parse, read_tap, render, tap_label, write_tap};
use crate::keyd::data::{PACKS, Pack, action_description};
use crate::keyd::rules::{
    binding_warnings, merge_pack, pack_summary, upsert_binding, warning_preview_lines,
    warnings_for_key,
};
use crate::keyd::system::{
    check, keyd_bin, keyd_etc_dir, read_live_config, run_apply,
};

/// Where the CLI reads and writes.
#[derive(Debug, Clone)]
pub struct Paths {
    /// The checkout's `default.conf`.
    pub conf: PathBuf,
    /// The checkout's `install.sh`.
    pub install: PathBuf,
    /// The live keyd directory.
    pub etc: PathBuf,
}

impl Paths {
    /// Paths from the environment, defaulting to the current directory.
    pub fn from_env() -> Self {
        Self {
            conf: std::env::var_os("CAPS_CONF")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("default.conf")),
            install: std::env::var_os("CAPS_INSTALL")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("install.sh")),
            etc: keyd_etc_dir(),
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "caps-gui",
    version,
    about = "Edit the keyd Caps Lock [nav] layer without the interactive UI.",
    long_about = "Commands only change the checkout's default.conf; add --apply to install afterwards."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Check the port's own invariants and exit
    #[arg(long)]
    pub selftest: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show bindings, tap action, and warnings
    List,
    /// Add or replace Caps + KEY → ACTION
    Add {
        /// keyd key name (for example h, volumedown)
        key: String,
        /// keyd action (for example left, C-minus)
        action: String,
        /// Install after saving
        #[arg(long)]
        apply: bool,
        /// Skip the apply confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Remove a Caps + KEY binding
    Rm {
        /// keyd key name
        key: String,
        /// Install after saving
        #[arg(long)]
        apply: bool,
        /// Skip the apply confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Set the [main] capslock tap action
    Tap {
        /// keyd action sent on a short Caps tap
        action: String,
        /// Install after saving
        #[arg(long)]
        apply: bool,
        /// Skip the apply confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// List the preset packs
    Packs,
    /// Merge a preset pack, by exact name or unique prefix
    Pack {
        /// Pack name or unique case-insensitive prefix
        name: String,
        /// Install after saving
        #[arg(long)]
        apply: bool,
        /// Skip the apply confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Diff the live config against default.conf
    Diff,
    /// Run keyd check on default.conf
    Check,
    /// Show the diff, confirm, and install with sudo
    Apply {
        /// Skip the confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

/// A user-facing failure carrying its exit code.
#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub code: i32,
}

impl CliError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), code: 1 }
    }

    pub fn with_code(message: impl Into<String>, code: i32) -> Self {
        Self { message: message.into(), code }
    }
}

/// Resolve a pack by exact or unique case-insensitive prefix.
pub fn resolve_pack<'a>(name: &str, packs: &'a [Pack]) -> (Option<&'a Pack>, Vec<&'a Pack>) {
    let query = name.trim().to_lowercase();
    if let Some(exact) = packs.iter().find(|pack| pack.name.to_lowercase() == query) {
        return (Some(exact), Vec::new());
    }
    let candidates: Vec<&Pack> = packs
        .iter()
        .filter(|pack| pack.name.to_lowercase().starts_with(&query))
        .collect();
    if candidates.len() == 1 {
        (Some(candidates[0]), Vec::new())
    } else {
        (None, candidates)
    }
}

fn load(paths: &Paths) -> Result<String, CliError> {
    std::fs::read_to_string(&paths.conf)
        .map_err(|error| CliError::new(format!("cannot read {}: {error}", paths.conf.display())))
}

fn save(paths: &Paths, text: &str) -> Result<(), CliError> {
    std::fs::write(&paths.conf, text)
        .map_err(|error| CliError::new(format!("cannot write {}: {error}", paths.conf.display())))
}

fn binding_line(key: &str, action: &str) -> String {
    match action_description(action) {
        Some(description) => format!("Caps + {key} → {action}  — {description}"),
        None => format!("Caps + {key} → {action}"),
    }
}

fn print_warnings(binds: &[crate::keyd::config::Binding], tap: Option<&str>) {
    for line in warning_preview_lines(binds, tap) {
        println!("{line}");
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        println!();
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

fn validate(text: &str) -> Result<(), CliError> {
    if keyd_bin().is_none() {
        eprintln!("keyd not installed; config not validated.");
        return Ok(());
    }
    match check(text) {
        Some(error) => Err(CliError::new(format!("keyd rejects the resulting config:\n{error}"))),
        None => Ok(()),
    }
}

fn apply_flow(paths: &Paths, yes: bool, text: Option<&str>) -> Result<i32, CliError> {
    let text = match text {
        Some(text) => text.to_string(),
        None => load(paths)?,
    };
    let live_path = paths.etc.join("default.conf");
    let (live, fallback) = read_live_config(&text);
    let lines = diff_lines(
        &live,
        &text,
        &format!("live {}", live_path.display()),
        &format!("repo {}", paths.conf.display()),
    );
    if fallback {
        eprintln!(
            "note: cannot read {}; the installer will create it.",
            live_path.display()
        );
    } else if lines.is_empty() {
        println!("No differences; nothing to apply.");
        return Ok(0);
    }
    for line in &lines {
        println!("{line}");
    }
    print_warnings(&parse(&text), read_tap(&text).action.as_deref());
    if !yes && !confirm("Apply these changes with sudo? [y/N] ") {
        println!("Cancelled; nothing applied.");
        return Ok(1);
    }
    let code = run_apply(&text, &paths.conf, &paths.install)
        .map_err(|error| CliError::new(error.to_string()))?;
    Ok(code)
}

struct Finish<'a> {
    paths: &'a Paths,
    apply: bool,
    yes: bool,
    key: Option<&'a str>,
}

fn finish(finish: Finish<'_>, text: &str, new_text: String, message: &str) -> Result<i32, CliError> {
    validate(&new_text)?;
    let changed = new_text != text;
    if changed {
        save(finish.paths, &new_text)?;
    }
    println!("{message}");
    if changed {
        println!("Saved {}.", finish.paths.conf.display());
    }
    let binds = parse(&new_text);
    let tap = read_tap(&new_text).action;
    match finish.key {
        Some(key) => {
            for (level, warning) in warnings_for_key(&binding_warnings(&binds, tap.as_deref()), key) {
                println!("{} {key}: {warning}", level.upper());
            }
        }
        None => print_warnings(&binds, tap.as_deref()),
    }
    if finish.apply {
        return apply_flow(finish.paths, finish.yes, Some(&new_text));
    }
    Ok(0)
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && !key.chars().any(char::is_whitespace) && !key.contains('=')
}

fn command(paths: &Paths, command: &Command) -> Result<i32, CliError> {
    match command {
        Command::List => {
            let text = load(paths)?;
            let binds = parse(&text);
            if binds.is_empty() {
                println!("No bindings in [nav].");
            }
            for binding in &binds {
                println!("{}", binding_line(&binding.key, &binding.action));
            }
            let tap = read_tap(&text);
            println!("Tap: {}", tap_label(&tap));
            print_warnings(&binds, tap.action.as_deref());
            Ok(0)
        }
        Command::Add { key, action, apply, yes } => {
            let key = key.trim().to_lowercase();
            let action = action.trim();
            if !valid_key(&key) {
                return Err(CliError::new(format!("invalid key name: {key:?}")));
            }
            if action.is_empty() || action.contains('\n') || action.contains('\r') {
                return Err(CliError::new(
                    "action must be a non-empty single-line keyd action.",
                ));
            }
            let text = load(paths)?;
            let binds = parse(&text);
            let new_text = render(&text, &upsert_binding(&binds, &key, action, None));
            let existing = binds.iter().find(|binding| binding.key == key);
            let message = match existing {
                None => format!("Added: {}", binding_line(&key, action)),
                Some(binding) if binding.action == action => {
                    format!("Already bound: {}", binding_line(&key, action))
                }
                Some(binding) => format!(
                    "Replaced: Caps + {key} → {action} (was {})",
                    binding.action
                ),
            };
            finish(
                Finish { paths, apply: *apply, yes: *yes, key: Some(&key) },
                &text,
                new_text,
                &message,
            )
        }
        Command::Rm { key, apply, yes } => {
            let key = key.trim().to_lowercase();
            if !valid_key(&key) {
                return Err(CliError::new(format!("invalid key name: {key:?}")));
            }
            let text = load(paths)?;
            let binds = parse(&text);
            let Some(index) = binds
                .iter()
                .position(|binding| binding.key.to_lowercase() == key)
            else {
                return Err(CliError::new(format!("not bound: {key}")));
            };
            let removed = binds[index].clone();
            let mut remaining = binds.clone();
            remaining.remove(index);
            let new_text = render(&text, &remaining);
            finish(
                Finish { paths, apply: *apply, yes: *yes, key: Some(&key) },
                &text,
                new_text,
                &format!("Removed: {}", binding_line(&removed.key, &removed.action)),
            )
        }
        Command::Tap { action, apply, yes } => {
            let text = load(paths)?;
            let new_text = write_tap(&text, action)
                .map_err(|error| CliError::new(error.to_string()))?;
            let tap = read_tap(&new_text);
            let message = if new_text == text {
                format!("Tap action already {}.", tap.action.clone().unwrap_or_default())
            } else {
                format!("Tap action: {}", tap_label(&tap))
            };
            finish(
                Finish { paths, apply: *apply, yes: *yes, key: None },
                &text,
                new_text,
                &message,
            )
        }
        Command::Packs => {
            for pack in PACKS {
                println!("{}  — {}", pack.name, pack.description);
            }
            Ok(0)
        }
        Command::Pack { name, apply, yes } => {
            let (pack, candidates) = resolve_pack(name, PACKS);
            let Some(pack) = pack else {
                if candidates.is_empty() {
                    return Err(CliError::with_code(
                        format!("no pack matches {name:?}; see 'caps-gui packs'."),
                        2,
                    ));
                }
                let listing = candidates
                    .iter()
                    .map(|candidate| format!("  {}", candidate.name))
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(CliError::with_code(
                    format!("pack {name:?} is ambiguous; candidates:\n{listing}"),
                    2,
                ));
            };
            let text = load(paths)?;
            let binds = parse(&text);
            let merge = merge_pack(&binds, pack);
            let new_text = render(&text, &merge.merged);
            let message = pack_summary(pack, &merge, read_tap(&text).action.as_deref()).join("\n");
            finish(
                Finish { paths, apply: *apply, yes: *yes, key: None },
                &text,
                new_text,
                &message,
            )
        }
        Command::Diff => {
            let text = load(paths)?;
            let live_path = paths.etc.join("default.conf");
            let (live, fallback) = read_live_config(&text);
            if fallback {
                eprintln!(
                    "note: cannot read {}; nothing to compare.",
                    live_path.display()
                );
                return Ok(2);
            }
            let lines = diff_lines(
                &live,
                &text,
                &format!("live {}", live_path.display()),
                &format!("repo {}", paths.conf.display()),
            );
            if lines.is_empty() {
                println!("No differences");
                return Ok(0);
            }
            for line in &lines {
                println!("{line}");
            }
            Ok(1)
        }
        Command::Check => {
            let text = load(paths)?;
            if keyd_bin().is_none() {
                return Err(CliError::new("keyd not installed; cannot validate."));
            }
            match check(&text) {
                Some(error) => {
                    eprintln!("{error}");
                    Err(CliError::new(format!("{} is invalid.", paths.conf.display())))
                }
                None => {
                    println!("{}: OK", paths.conf.display());
                    Ok(0)
                }
            }
        }
        Command::Apply { yes } => apply_flow(paths, *yes, None),
    }
}

/// Run the CLI against explicit paths; returns the process exit code.
pub fn run_with(paths: &Paths, cli: &Cli) -> i32 {
    if cli.selftest {
        return selftest(paths);
    }
    let Some(subcommand) = &cli.command else {
        let mut command = <Cli as clap::CommandFactory>::command();
        let _ = command.print_help();
        println!();
        return 2;
    };
    match command(paths, subcommand) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("caps-gui: {}", error.message);
            error.code
        }
    }
}

/// Run the CLI against the environment's paths.
pub fn run(cli: &Cli) -> i32 {
    run_with(&Paths::from_env(), cli)
}

/// The port's own invariants: the config round-trips, macros round-trip, packs merge, and every
/// catalog action is one keyd accepts when keyd is installed.
fn selftest(paths: &Paths) -> i32 {
    let mut failures = 0;
    let mut check_that = |ok: bool, what: &str| {
        if ok {
            println!("ok   {what}");
        } else {
            println!("FAIL {what}");
            failures += 1;
        }
    };

    let text = match std::fs::read_to_string(&paths.conf) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("caps-gui: cannot read {}: {error}", paths.conf.display());
            return 1;
        }
    };
    let binds = parse(&text);
    check_that(!binds.is_empty(), "default.conf has [nav] bindings");
    check_that(render(&text, &binds) == render(&text, &parse(&render(&text, &binds))), "render round-trips");
    check_that(read_tap(&text).editable, "the tap action is editable");

    for pack in PACKS {
        let merge = merge_pack(&binds, pack);
        check_that(
            merge.merged.len() >= binds.len(),
            &format!("pack {} merges without losing bindings", pack.name),
        );
    }

    for steps in [
        vec![crate::keyd::macros::MacroStep::Text("hi there".into())],
        vec![crate::keyd::macros::MacroStep::Key("C-a".into())],
        vec![crate::keyd::macros::MacroStep::Delay(50)],
    ] {
        let action = crate::keyd::macros::build_macro(&steps).expect("build");
        check_that(
            crate::keyd::macros::parse_macro(&action).is_some(),
            &format!("macro {action} parses"),
        );
    }

    if keyd_bin().is_some() {
        check_that(check(&text).is_none(), "keyd accepts default.conf");
        for entry in crate::keyd::data::CATALOG {
            let probe = format!("[nav]\na = {}\n", entry.action);
            check_that(
                check(&probe).is_none(),
                &format!("keyd accepts catalog action {}", entry.action),
            );
        }
    } else {
        println!("skip keyd checks (keyd not installed)");
    }

    if failures == 0 {
        println!("selftest ok");
        0
    } else {
        println!("selftest FAILED ({failures})");
        1
    }
}
