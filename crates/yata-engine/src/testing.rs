//! Tests of rules with fixtures (`yata rules test`).
//!
//! [`run_tests`] loads policy directories with their fixture directories and checks, for every
//! rule, that the fixtures exist, are well formed, use only dummy account IDs and that the rule
//! produces the failures each fixture expects. The outcome is a [`TestReport`], which can be
//! serialized to the `result`, `summary`, `policies`, `rules` and `failures` parts of the JSON
//! output of `yata rules test` (the caller adds `schema_version` and `tool`).

mod dummy;
mod expected;
mod fixture;
mod report;
mod runner;

pub use dummy::DUMMY_ACCOUNT_IDS;
pub use report::{
    FixtureCounts, FixtureResult, FixtureStatus, RuleCounts, RuleTestResult, RuleTestStatus,
    TestReport, TestSummary,
};
pub use runner::run_tests;
