//! The `CollectContext` given to collectors.

// Used by the service collectors that are added in later changes.
#![allow(dead_code)]

use crate::support::clients::{ClientFactory, GuardedClient};

/// What a collector may use: the account and guarded clients.
///
/// The SDK configuration is deliberately not exposed, so collectors cannot create
/// unguarded clients.
#[derive(Debug)]
pub(crate) struct CollectContext {
    account_id: String,
    account_key: String,
    factory: ClientFactory,
}

impl CollectContext {
    /// Creates a context for one collection run.
    pub(crate) fn new(account_id: String, account_key: String, factory: ClientFactory) -> Self {
        Self {
            account_id,
            account_key,
            factory,
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

    /// Creates a read-only guarded client of type `C` that sends requests to `region`.
    pub(crate) fn client<C: GuardedClient>(&self, region: &str) -> C {
        self.factory.client::<C>(region)
    }
}
