use clap::Parser;

use caps_gui::cli::Cli;

/// Rune parses its own headless flags (`--snap`, `--tree`, `--script`, …) out of argv and hands
/// back only what it did not recognise, so the CLI never has to know them.
#[cfg(feature = "ui")]
fn main() {
    let argv: Vec<String> = std::iter::once("caps-gui".to_string())
        .chain(rune::extra_args().iter().cloned())
        .collect();
    let cli = Cli::parse_from(argv);
    if cli.command.is_some() || cli.selftest {
        std::process::exit(caps_gui::cli::run(&cli));
    }
    caps_gui::ui::run();
}

#[cfg(not(feature = "ui"))]
fn main() {
    let cli = Cli::parse();
    std::process::exit(caps_gui::cli::run(&cli));
}
