//! The `CollectContext` given to collectors.

// Used by the service collectors that are added in later changes.
#![allow(dead_code)]

use std::num::NonZeroUsize;

use crate::support::clients::{ClientFactory, GuardedClient};

/// Maximum number of tasks that run at once inside one collection group (see `fan_out`).
pub(crate) const FAN_OUT_LIMIT: usize = 8;

/// What a collector may use: the account and guarded clients.
///
/// The SDK configuration is deliberately not exposed, so collectors cannot create
/// unguarded clients.
#[derive(Debug)]
pub(crate) struct CollectContext {
    account_id: String,
    account_key: String,
    factory: ClientFactory,
    fan_out_limit: NonZeroUsize,
}

impl CollectContext {
    /// Creates a context for one collection run.
    pub(crate) fn new(account_id: String, account_key: String, factory: ClientFactory) -> Self {
        Self {
            account_id,
            account_key,
            factory,
            fan_out_limit: NonZeroUsize::new(FAN_OUT_LIMIT).unwrap_or(NonZeroUsize::MIN),
        }
    }

    /// Returns the account id (12 digits).
    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Returns the account key (`aws-<account id>`).
    pub(crate) fn account_key(&self) -> &str {
        &self.account_key
    }

    /// Returns the maximum number of concurrent tasks inside one collection group.
    pub(crate) fn fan_out_limit(&self) -> NonZeroUsize {
        self.fan_out_limit
    }

    /// Returns this context with a different fan-out limit (tests only).
    #[cfg(test)]
    pub(crate) fn with_fan_out_limit(mut self, limit: NonZeroUsize) -> Self {
        self.fan_out_limit = limit;
        self
    }

    /// Creates a read-only guarded client of type `C` that sends requests to `region`.
    pub(crate) fn client<C: GuardedClient>(&self, region: &str) -> C {
        self.factory.client::<C>(region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::clients::testing::mock_factory;

    #[test]
    fn fan_out_limit_defaults_to_eight_and_can_be_changed_in_tests() {
        assert_eq!(FAN_OUT_LIMIT, 8);
        let ctx = CollectContext::new(
            "123456789012".to_owned(),
            "aws-123456789012".to_owned(),
            mock_factory(&[]),
        );
        assert_eq!(ctx.fan_out_limit().get(), 8);
        let one = NonZeroUsize::MIN;
        assert_eq!(ctx.with_fan_out_limit(one).fan_out_limit(), one);
    }
}
