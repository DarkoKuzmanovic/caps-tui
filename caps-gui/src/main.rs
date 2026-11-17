use clap::Parser;

use caps_gui::cli::Cli;

/// Rune's headless tooling flags. They belong to `rune::run`, not to this CLI, so their presence
/// means "open the app and let Rune handle the run" rather than "parse a subcommand".
const TOOLING_FLAGS: [&str; 25] = [
    "--snap",
    "--tree",
    "--script",
    "--video",
    "--frames",
    "--at",
    "--scale",
    "--size",
    "--theme",
    "--scene",
    "--list-scenes",
    "--trace",
    "--out",
    "--start",
    "--every",
    "--cols",
    "--strip",
    "--crop",
    "--keys",
    "--pointer",
    "--desktop",
    "--duration",
    "--fps",
    "--native",
    "--chrome",
];

fn wants_tooling(args: impl Iterator<Item = String>) -> bool {
    args.into_iter().any(|arg| {
        TOOLING_FLAGS
            .iter()
            .any(|flag| arg == *flag || arg.starts_with(&format!("{flag}=")))
    })
}

fn main() {
    let tooling = wants_tooling(std::env::args().skip(1));
    if !tooling {
        let cli = Cli::parse();
        // No subcommand and no --selftest: open the window.
        if cli.command.is_some() || cli.selftest {
            std::process::exit(caps_gui::cli::run(&cli));
        }
    }
    caps_gui::ui::run();
}
