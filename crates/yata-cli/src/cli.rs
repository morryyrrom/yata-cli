//! Command-line definition (clap). Subcommands are listed in [`Command`]; the processing of each
//! one lives in its own module under `commands/`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// The `yata` command line.
#[derive(Debug, Parser)]
#[command(name = "yata", version, about = "Cloud security posture checks", long_about = None)]
pub struct Cli {
    /// Options shared by all subcommands.
    #[command(flatten)]
    pub global: GlobalArgs,
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Options accepted by every subcommand, before or after the subcommand name.
#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Increase log verbosity (-v: info, -vv: debug, -vvv: trace). Overrides YATA_LOG.
    #[arg(
        short = 'v',
        long = "verbose",
        action = clap::ArgAction::Count,
        global = true,
        conflicts_with = "quiet"
    )]
    pub verbose: u8,
    /// Log errors only. Overrides YATA_LOG.
    #[arg(short = 'q', long = "quiet", global = true)]
    pub quiet: bool,
    /// When to use colors (default: auto; the NO_COLOR environment variable disables colors
    /// unless this option is given).
    #[arg(long = "color", value_enum, global = true, value_name = "WHEN")]
    pub color: Option<ColorMode>,
}

/// Value of `--color`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    /// Use colors when the output is a terminal.
    Auto,
    /// Always use colors.
    Always,
    /// Never use colors.
    Never,
}

impl ColorMode {
    /// Decides whether to use colors.
    ///
    /// `option` is the value of `--color`, `no_color_set` tells whether the `NO_COLOR`
    /// environment variable is set (whatever its value) and `is_terminal` whether the output is
    /// a terminal. The option wins over the environment variable, which wins over the default
    /// (`auto`).
    pub fn enabled(option: Option<ColorMode>, no_color_set: bool, is_terminal: bool) -> bool {
        match option {
            Some(ColorMode::Always) => true,
            Some(ColorMode::Never) => false,
            Some(ColorMode::Auto) => is_terminal,
            None => !no_color_set && is_terminal,
        }
    }
}

/// The top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Commands about policies (rules).
    #[command(subcommand)]
    Rules(RulesCommand),
}

/// Subcommands of `yata rules`.
#[derive(Debug, Subcommand)]
pub enum RulesCommand {
    /// Check the rules in policy directories: manifest, loading, fixtures, dummy values and
    /// provider support.
    Test(RulesTestArgs),
}

/// Options for selecting policy directories.
#[derive(Debug, Clone, Args)]
pub struct PolicyDirArgs {
    /// Policy directory with a `yata-policies.json` manifest (repeatable). Overrides
    /// YATA_POLICY_DIR.
    #[arg(long = "policy-dir", value_name = "DIR")]
    pub policy_dir: Vec<PathBuf>,
}

/// Arguments of `yata rules test`.
#[derive(Debug, Clone, Args)]
pub struct RulesTestArgs {
    /// Policy directories.
    #[command(flatten)]
    pub policy: PolicyDirArgs,
    /// Output format of the result on standard output.
    #[arg(long = "format", value_enum, default_value_t = RulesTestFormat::Text)]
    pub format: RulesTestFormat,
    /// Time limit of one evaluation in seconds (1-3600).
    #[arg(
        long = "policy-timeout",
        value_name = "SECONDS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..=3600)
    )]
    pub policy_timeout: u64,
}

/// Value of `--format` of `yata rules test`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RulesTestFormat {
    /// Human-readable text (the format is not stable).
    Text,
    /// Machine-readable JSON.
    Json,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Takes the arguments of `rules test` out of a parsed command.
    #[allow(unreachable_patterns)] // Other subcommands will make the fallback reachable.
    fn rules_test_args(command: Command) -> RulesTestArgs {
        match command {
            Command::Rules(RulesCommand::Test(args)) => args,
            other => panic!("expected `rules test`, got {other:?}"),
        }
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("yata").chain(args.iter().copied()))
    }

    #[test]
    fn color_option_beats_environment() {
        use ColorMode::*;
        assert!(ColorMode::enabled(Some(Always), true, false));
        assert!(!ColorMode::enabled(Some(Never), false, true));
        assert!(ColorMode::enabled(Some(Auto), true, true));
        assert!(!ColorMode::enabled(Some(Auto), false, false));
        assert!(!ColorMode::enabled(None, true, true));
        assert!(ColorMode::enabled(None, false, true));
        assert!(!ColorMode::enabled(None, false, false));
    }

    #[test]
    fn rules_test_defaults() {
        let cli = parse(&["rules", "test"]).unwrap();
        let args = rules_test_args(cli.command);
        assert!(args.policy.policy_dir.is_empty());
        assert_eq!(args.format, RulesTestFormat::Text);
        assert_eq!(args.policy_timeout, 30);
        assert_eq!(cli.global.verbose, 0);
        assert!(!cli.global.quiet);
        assert_eq!(cli.global.color, None);
    }

    #[test]
    fn global_options_are_accepted_after_the_subcommand() {
        let cli = parse(&[
            "rules",
            "test",
            "-vv",
            "--color",
            "never",
            "--policy-dir",
            "a",
            "--policy-dir",
            "b",
        ])
        .unwrap();
        assert_eq!(cli.global.verbose, 2);
        assert_eq!(cli.global.color, Some(ColorMode::Never));
        let args = rules_test_args(cli.command);
        assert_eq!(args.policy.policy_dir.len(), 2);
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(parse(&["rules", "test", "--format", "table"]).is_err());
        assert!(parse(&["rules", "test", "--policy-timeout", "0"]).is_err());
        assert!(parse(&["rules", "test", "--policy-timeout", "3601"]).is_err());
        assert!(parse(&["rules", "test", "--policy-timeout", "3600"]).is_ok());
        assert!(parse(&["rules", "test", "-v", "-q"]).is_err());
    }
}
