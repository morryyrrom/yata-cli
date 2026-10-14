#![forbid(unsafe_code)]
//! Command-line entry point for Yata Cloud (the `yata` binary).

mod cli;
mod commands;
mod exit_code;
mod logging;
mod policy_dir;
mod sanitize;

use std::io::{IsTerminal, Write};

use clap::Parser;

use crate::cli::{Cli, ColorMode};
use crate::commands::CommandContext;
use crate::exit_code::CliError;

fn main() {
    let code = run();
    std::process::exit(code);
}

/// Parses the arguments, runs the command and returns the exit code.
fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        // Prints the message (to standard output for --help and --version, otherwise to
        // standard error) and exits with 0 or 2.
        Err(err) => err.exit(),
    };

    let no_color = std::env::var_os("NO_COLOR").is_some();
    let stderr_color =
        ColorMode::enabled(cli.global.color, no_color, std::io::stderr().is_terminal());
    let yata_log = std::env::var_os("YATA_LOG");
    if let Err(err) = logging::init(&cli.global, yata_log.as_deref(), stderr_color) {
        return print_error(&err);
    }

    let context = CommandContext {
        policy_dir_env: std::env::var_os("YATA_POLICY_DIR"),
        stdout_color: ColorMode::enabled(
            cli.global.color,
            no_color,
            std::io::stdout().is_terminal(),
        ),
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            return print_error(&CliError::Runtime(format!(
                "failed to start the async runtime: {err}"
            )));
        }
    };
    let result = runtime.block_on(commands::run(cli.command, &context));
    // Do not wait for blocking tasks that might still run.
    runtime.shutdown_background();
    match result {
        Ok(code) => code,
        Err(err) => print_error(&err),
    }
}

/// Writes an error to standard error and returns its exit code.
fn print_error(err: &CliError) -> i32 {
    let mut stderr = std::io::stderr().lock();
    // Nothing more can be done if standard error is closed.
    let _ = writeln!(stderr, "error: {err}");
    let _ = stderr.flush();
    err.exit_code()
}
