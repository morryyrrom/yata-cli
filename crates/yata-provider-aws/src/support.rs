//! Helpers shared by collectors: client creation, the read-only guard and error mapping.

pub(crate) mod clients;
pub(crate) mod convert;
pub(crate) mod errors;
pub(crate) mod fan_out;
#[cfg(test)]
pub(crate) mod golden;
pub(crate) mod guard;
pub(crate) mod guard_lists;
pub(crate) mod partition;
