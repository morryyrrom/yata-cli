//! The `Collector` trait implemented by every resource-type collector.

use std::future::Future;
use std::pin::Pin;

use yata_core::{Resource, Scope};

use crate::context::CollectContext;
use crate::support::errors::CollectError;

/// Boxed future returned by [`Collector::collect`].
pub(crate) type CollectFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Resource>, CollectError>> + Send + 'a>>;

/// Collects the resources of one resource type.
///
/// A collector only gathers information: it calls APIs with their default parameters
/// (except paging tokens and required identifiers) and keeps the results as they are.
/// API calls inside a collector are made one after another. The exception is the
/// per-resource follow-up calls, which may run concurrently through
/// [`crate::support::fan_out::fan_out`] with `CollectContext::fan_out_limit()` as the limit.
pub(crate) trait Collector: Send + Sync {
    /// Resource type produced by this collector, e.g. `aws.s3.bucket`.
    fn resource_type(&self) -> &'static str;

    /// Whether the resource is global or regional.
    fn scope(&self) -> Scope;

    /// IAM actions required by this collector, e.g. `["s3:ListAllMyBuckets"]`.
    fn required_permissions(&self) -> &'static [&'static str];

    /// Collects resources by calling APIs in `region`.
    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a>;
}
