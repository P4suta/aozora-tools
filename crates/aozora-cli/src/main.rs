use std::process::ExitCode;

use clap::Parser;

use aozora_cli::Cli;

fn main() -> ExitCode {
    aozora_cli::run(Cli::parse())
}
