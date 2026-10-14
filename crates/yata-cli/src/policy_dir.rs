//! Decides the policy directories: `--policy-dir` over `YATA_POLICY_DIR` (no default).

use std::ffi::OsString;
use std::path::PathBuf;

use crate::exit_code::CliError;

/// Returns the policy directories to use.
///
/// If `--policy-dir` was given at least once, only its values are used and `env_value` (the
/// value of `YATA_POLICY_DIR`) is ignored; the two are never mixed. Otherwise `env_value` is
/// split with the path separator of the OS (`:` or `;`). Empty entries are dropped, so an empty
/// variable counts as unset. With neither, the result is a usage error (exit code 2).
pub fn resolve(
    option_values: &[PathBuf],
    env_value: Option<OsString>,
) -> Result<Vec<PathBuf>, CliError> {
    if !option_values.is_empty() {
        return Ok(option_values.to_vec());
    }
    let from_env: Vec<PathBuf> = env_value
        .iter()
        .flat_map(std::env::split_paths)
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    if from_env.is_empty() {
        return Err(CliError::Usage(
            "no policy directory given: use --policy-dir <DIR> or set YATA_POLICY_DIR".to_owned(),
        ));
    }
    Ok(from_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn option_wins_and_is_not_mixed_with_the_environment() {
        let env = std::env::join_paths([p("env_a"), p("env_b")]).unwrap();
        let dirs = resolve(&[p("opt")], Some(env)).unwrap();
        assert_eq!(dirs, vec![p("opt")]);
    }

    #[test]
    fn environment_is_split_by_the_os_separator() {
        let env = std::env::join_paths([p("a"), p("b")]).unwrap();
        assert_eq!(resolve(&[], Some(env)).unwrap(), vec![p("a"), p("b")]);
    }

    #[test]
    fn nothing_given_is_a_usage_error() {
        assert!(matches!(resolve(&[], None), Err(CliError::Usage(_))));
        assert!(matches!(
            resolve(&[], Some(OsString::new())),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn repeated_options_keep_their_order() {
        let dirs = resolve(&[p("b"), p("a")], None).unwrap();
        assert_eq!(dirs, vec![p("b"), p("a")]);
    }
}
