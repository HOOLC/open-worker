use clap::Parser;

fn main() {
    let cli = zork_call::Cli::parse();
    if let Err(error) = zork_call::run(cli) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
