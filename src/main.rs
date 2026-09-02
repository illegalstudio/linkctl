//! `linkctl` — native Linux command-line control for Insta360 Link webcams.

mod camera;
mod cli;
mod commands;
mod config;
mod error;
mod output;
mod presets;
mod preview;
mod units;

use clap::Parser;

use crate::error::ExitCode;

fn main() {
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // clap prints help/version to stdout with exit 0, and usage
            // errors to stderr; we only override the error exit code.
            let code = if e.use_stderr() {
                ExitCode::InvalidArguments as i32
            } else {
                0
            };
            let _ = e.print();
            std::process::exit(code);
        }
    };
    let out = output::Output::new(cli.json, cli.quiet, cli.verbose);
    match commands::run(cli) {
        Ok(()) => {}
        Err(err) => {
            out.error(&err);
            std::process::exit(err.exit_code() as i32);
        }
    }
}
