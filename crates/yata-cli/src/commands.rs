//! Subcommand implementations: one module per subcommand (`<name>.rs`, with a `<name>/`
//! directory for submodules when needed).

use std::ffi::OsString;

use crate::cli::{Command, RulesCommand};
use crate::exit_code::CliError;

/// `yata rules test`.
pub mod rules_test;

/// Settings that come from the environment of the process, resolved once in `main`.
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// Value of `YATA_POLICY_DIR`, if set.
    pub policy_dir_env: Option<OsString>,
    /// Whether to use colors on standard output (`--color`, `NO_COLOR`, terminal detection).
    pub stdout_color: bool,
}

/// Runs the chosen subcommand and returns its exit code.
///
/// A new subcommand adds its module above and one arm here.
pub async fn run(command: Command, context: &CommandContext) -> Result<i32, CliError> {
    match command {
        Command::Rules(RulesCommand::Test(args)) => rules_test::run(&args, context).await,
    }
}
