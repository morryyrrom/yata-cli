//! Log initialization: `-v` / `-q` / `YATA_LOG` (option over environment variable over default).

use std::ffi::OsStr;

use tracing_subscriber::EnvFilter;

use crate::cli::GlobalArgs;
use crate::exit_code::CliError;

/// Default filter: warnings and errors from every crate.
const DEFAULT_SPEC: &str = "warn";

/// Builds the `EnvFilter` directive string.
///
/// `-v` / `-q` change the level of the `yata*` crates only (dependencies stay at `warn`) and
/// make `YATA_LOG` unused. Without them, `YATA_LOG` is used as it is; without it, `warn`.
pub fn filter_spec(verbose: u8, quiet: bool, yata_log: Option<&str>) -> String {
    if quiet {
        return format!("{DEFAULT_SPEC},yata=error");
    }
    if verbose > 0 {
        let level = match verbose {
            1 => "info",
            2 => "debug",
            _ => "trace",
        };
        return format!("{DEFAULT_SPEC},yata={level}");
    }
    match yata_log {
        Some(spec) if !spec.trim().is_empty() => spec.to_owned(),
        _ => DEFAULT_SPEC.to_owned(),
    }
}

/// Installs the log subscriber, which writes human-readable lines to standard error.
///
/// `yata_log` is the value of `YATA_LOG`. A `YATA_LOG` that is not a valid filter is a usage
/// error (it is not read when `-v` or `-q` is given).
pub fn init(global: &GlobalArgs, yata_log: Option<&OsStr>, ansi: bool) -> Result<(), CliError> {
    // `YATA_LOG` is not read when `-v` or `-q` is given, so its value does not matter then.
    let yata_log = if global.verbose > 0 || global.quiet {
        None
    } else {
        yata_log
            .map(|value| {
                value
                    .to_str()
                    .ok_or_else(|| CliError::Usage("YATA_LOG is not valid UTF-8".to_owned()))
            })
            .transpose()?
    };
    let spec = filter_spec(global.verbose, global.quiet, yata_log);
    let filter = EnvFilter::try_new(&spec)
        .map_err(|err| CliError::Usage(format!("invalid YATA_LOG {spec:?}: {err}")))?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(ansi)
        .try_init()
        .map_err(|err| CliError::Runtime(format!("failed to initialize logging: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_warn() {
        assert_eq!(filter_spec(0, false, None), "warn");
        assert_eq!(filter_spec(0, false, Some("")), "warn");
    }

    #[test]
    fn verbosity_levels() {
        assert_eq!(filter_spec(1, false, None), "warn,yata=info");
        assert_eq!(filter_spec(2, false, None), "warn,yata=debug");
        assert_eq!(filter_spec(3, false, None), "warn,yata=trace");
        assert_eq!(filter_spec(9, false, None), "warn,yata=trace");
        assert_eq!(filter_spec(0, true, None), "warn,yata=error");
    }

    #[test]
    fn options_win_over_yata_log() {
        assert_eq!(filter_spec(1, false, Some("trace")), "warn,yata=info");
        assert_eq!(filter_spec(0, true, Some("trace")), "warn,yata=error");
        assert_eq!(filter_spec(0, false, Some("yata=debug")), "yata=debug");
    }

    #[test]
    fn the_specs_are_valid_filters() {
        for spec in [
            filter_spec(1, false, None),
            filter_spec(0, true, None),
            filter_spec(0, false, None),
        ] {
            assert!(EnvFilter::try_new(&spec).is_ok(), "{spec}");
        }
        assert!(EnvFilter::try_new("yata=nonsense=").is_err());
    }
}
