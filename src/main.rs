use std::process::ExitCode;

mod cli;
mod cli_tracing;
mod tui;

fn main() -> ExitCode {
    cli::run_main()
}
