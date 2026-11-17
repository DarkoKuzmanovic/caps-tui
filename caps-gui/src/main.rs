use clap::Parser;

fn main() {
    let cli = caps_gui::cli::Cli::parse();
    std::process::exit(caps_gui::cli::run(&cli));
}
