//! Exit codes and the mapping from errors to exit codes.

use std::fmt;
use std::io::Write;

/// Everything succeeded.
pub const SUCCESS: i32 = 0;
/// `yata rules test`: at least one rule-level failure.
pub const RULE_FAILURES: i32 = 1;
/// Usage or input error (same code as clap uses for argument errors).
pub const USAGE: i32 = 2;
/// Runtime error, for example a failed write to standard output.
pub const RUNTIME: i32 = 3;
/// Interrupted by Ctrl-C.
pub const INTERRUPTED: i32 = 130;

/// An error that ends a command without a result.
#[derive(Debug)]
pub enum CliError {
    /// A usage or input error. The message may span several lines.
    Usage(String),
    /// Writing the result failed.
    Output(std::io::Error),
    /// An unexpected runtime failure.
    Runtime(String),
}

impl CliError {
    /// The exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage(_) => USAGE,
            CliError::Output(_) | CliError::Runtime(_) => RUNTIME,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(message) => f.write_str(message),
            CliError::Output(err) => write!(f, "failed to write the output: {err}"),
            CliError::Runtime(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CliError {}

/// Writes the interruption notice to standard error, flushes it and ends the process with
/// [`INTERRUPTED`].
///
/// `std::process::exit` does not wait for threads that are still running (for example a
/// blocking task of the tokio runtime), unlike dropping the runtime.
pub fn exit_interrupted() -> ! {
    let mut stderr = std::io::stderr().lock();
    // Nothing more can be done if standard error is closed.
    let _ = writeln!(stderr, "interrupted");
    let _ = stderr.flush();
    std::process::exit(INTERRUPTED)
}

/// The notification arrived before the work finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupted;

/// Waits for `work` and `interrupt` at the same time.
///
/// Returns the output of `work`, or [`Interrupted`] when `interrupt` completes first (also when
/// both are ready: the interruption wins). The notification is a parameter so that tests do not
/// need a real signal.
pub async fn run_or_interrupt<T>(
    work: impl Future<Output = T>,
    interrupt: impl Future<Output = ()>,
) -> Result<T, Interrupted> {
    tokio::select! {
        biased;
        () = interrupt => Err(Interrupted),
        output = work => Ok(output),
    }
}

/// Completes when Ctrl-C is pressed. If the handler cannot be installed, it never completes
/// (the default behaviour of Ctrl-C then still ends the process).
pub async fn ctrl_c() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::warn!("cannot listen for Ctrl-C: {err}");
        std::future::pending::<()>().await;
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, ready};

    use super::*;

    #[tokio::test]
    async fn interruption_wins_over_work_that_never_ends() {
        let result = run_or_interrupt(pending::<u32>(), ready(())).await;
        assert_eq!(result, Err(Interrupted));
    }

    #[tokio::test]
    async fn interruption_wins_when_both_are_ready() {
        let result = run_or_interrupt(ready(1), ready(())).await;
        assert_eq!(result, Err(Interrupted));
    }

    #[tokio::test]
    async fn finished_work_is_returned() {
        let result = run_or_interrupt(ready(7), pending::<()>()).await;
        assert_eq!(result, Ok(7));
    }

    #[tokio::test]
    async fn blocking_work_that_has_not_finished_is_abandoned() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let work = tokio::task::spawn_blocking(move || wait.recv().is_ok());
        let result = run_or_interrupt(work, ready(())).await;
        assert!(result.is_err());
        // Let the blocking thread end.
        let _ = release.send(());
    }

    #[test]
    fn errors_map_to_exit_codes() {
        assert_eq!(CliError::Usage("x".into()).exit_code(), 2);
        assert_eq!(
            CliError::Output(std::io::Error::other("closed")).exit_code(),
            3
        );
        assert_eq!(CliError::Runtime("x".into()).exit_code(), 3);
    }
}
