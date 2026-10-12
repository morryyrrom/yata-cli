//! Execution of a collection: group expansion, concurrency, cancellation and assembly.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use yata_core::{
    AccountInfo, CollectionError, CollectionErrorKind, Inventory, Resource, ResourceSet,
    ResourceTypeInfo, Scope,
};

use crate::ACCOUNT_RESOURCE_TYPE;
use crate::collector::Collector;
use crate::context::CollectContext;
use crate::error::ProviderError;
use crate::setup::{Identity, discover_regions, identify};
use crate::support::clients::ClientFactory;
use crate::support::errors::CollectError;
use crate::support::partition::DEFAULT_GLOBAL_REGION;

/// Region label of a global group.
const GLOBAL_REGION: &str = "global";

/// A validated collection request.
#[derive(Debug, Clone)]
pub(crate) struct RunParams {
    /// Requested resource types (deduplicated). May include the account type.
    pub(crate) resource_types: BTreeSet<String>,
    /// Target regions (validated, deduplicated, ascending), or `None` to discover them.
    pub(crate) regions: Option<Vec<String>>,
    /// Maximum number of groups running at the same time (at least 1).
    pub(crate) concurrency: usize,
}

/// One unit of work: a collector run for one region (or `global`).
struct Group {
    collector: Arc<dyn Collector>,
    resource_type: &'static str,
    scope: Scope,
    /// `global` or a region name; recorded in the result.
    group_region: String,
    /// Region passed to the collector.
    api_region: String,
}

/// Identifies a group in results and logs.
#[derive(Debug, Clone)]
struct GroupInfo {
    account: String,
    resource_type: &'static str,
    scope: Scope,
    region: String,
}

/// How a group ended.
#[derive(Debug)]
pub(crate) enum GroupOutcome {
    /// The group was collected.
    Collected(ResourceSet),
    /// The group failed.
    Failed(CollectionError),
}

/// Expands the collectors into groups in the execution order: global groups by type name,
/// then regions ascending times types ascending.
fn expand_groups(
    collectors: &BTreeMap<&'static str, Arc<dyn Collector>>,
    regions: &[String],
) -> Vec<Group> {
    let mut groups = Vec::new();
    for (resource_type, collector) in collectors {
        if collector.scope() == Scope::Global {
            groups.push(Group {
                collector: Arc::clone(collector),
                resource_type,
                scope: Scope::Global,
                group_region: GLOBAL_REGION.to_owned(),
                api_region: DEFAULT_GLOBAL_REGION.to_owned(),
            });
        }
    }
    for region in regions {
        for (resource_type, collector) in collectors {
            if collector.scope() == Scope::Regional {
                groups.push(Group {
                    collector: Arc::clone(collector),
                    resource_type,
                    scope: Scope::Regional,
                    group_region: region.clone(),
                    api_region: region.clone(),
                });
            }
        }
    }
    groups
}

/// Turns the collector's result into the outcome of the group.
///
/// A result is invalid (and the group fails with `invalid_response`) when a resource has a
/// different type or account, when an id appears twice, or when a resource of a regional
/// type has a region other than the group's region.
fn route_result(info: &GroupInfo, result: Result<Vec<Resource>, CollectError>) -> GroupOutcome {
    let failed = |kind: CollectionErrorKind, message: String| {
        GroupOutcome::Failed(CollectionError {
            account: info.account.clone(),
            region: info.region.clone(),
            resource_type: info.resource_type.to_owned(),
            kind,
            message,
        })
    };
    let mut resources = match result {
        Ok(resources) => resources,
        Err(err) => return failed(err.kind(), err.message().to_owned()),
    };

    let invalid = |message: &str| failed(CollectionErrorKind::InvalidResponse, message.to_owned());
    let mut ids = BTreeSet::new();
    for resource in &resources {
        if resource.resource_type != info.resource_type {
            return invalid(
                "collector returned a resource whose type differs from the collector's type",
            );
        }
        if resource.account != info.account {
            return invalid(
                "collector returned a resource whose account differs from the account key",
            );
        }
        if info.scope == Scope::Regional && resource.region != info.region {
            return invalid(
                "collector returned a resource whose region differs from the group's region",
            );
        }
        if !ids.insert(resource.id.as_str()) {
            return invalid("collector returned two resources with the same id");
        }
    }
    resources.sort_by(|a, b| a.id.cmp(&b.id));
    GroupOutcome::Collected(ResourceSet {
        account: info.account.clone(),
        region: info.region.clone(),
        resource_type: info.resource_type.to_owned(),
        resources,
    })
}

fn collector_map(
    collectors: Vec<Box<dyn Collector>>,
) -> BTreeMap<&'static str, Arc<dyn Collector>> {
    collectors
        .into_iter()
        .map(|collector| (collector.resource_type(), Arc::from(collector)))
        .collect()
}

/// Runs the collection after the SDK configuration has been loaded.
///
/// `cancel` completing aborts every running task and returns [`ProviderError::Interrupted`].
pub(crate) async fn run<F>(
    params: RunParams,
    factory: ClientFactory,
    first_region: &str,
    collectors: Vec<Box<dyn Collector>>,
    cancel: &mut F,
) -> Result<Inventory, ProviderError>
where
    F: Future<Output = ()> + Unpin,
{
    let started_at = SystemTime::now();
    let started = Instant::now();

    let identity = tokio::select! {
        biased;
        () = &mut *cancel => return Err(ProviderError::Interrupted),
        identity = identify(&factory, first_region) => identity?,
    };

    let regions = match &params.regions {
        Some(regions) => {
            let mut regions = regions.clone();
            regions.sort();
            regions.dedup();
            regions
        }
        None => tokio::select! {
            biased;
            () = &mut *cancel => return Err(ProviderError::Interrupted),
            regions = discover_regions(&factory, first_region) => regions?,
        },
    };

    let mut selected = collector_map(collectors);
    selected.retain(|resource_type, _| params.resource_types.contains(*resource_type));
    let groups = expand_groups(&selected, &regions);
    tracing::info!(
        regions = regions.len(),
        region_names = ?regions,
        groups = groups.len(),
        "starting the collection"
    );

    let context = Arc::new(CollectContext::new(
        identity.account_id.clone(),
        identity.account_key.clone(),
        factory,
    ));
    let (mut sets, mut errors) =
        run_groups(groups, &identity, &context, params.concurrency, cancel).await?;

    sets.push(ResourceSet {
        account: identity.account_key.clone(),
        region: GLOBAL_REGION.to_owned(),
        resource_type: ACCOUNT_RESOURCE_TYPE.to_owned(),
        resources: vec![identity.resource.clone()],
    });
    sets.sort_by(|a, b| {
        (&a.account, &a.region, &a.resource_type).cmp(&(&b.account, &b.region, &b.resource_type))
    });
    errors.sort_by(|a, b| {
        (&a.account, &a.region, &a.resource_type).cmp(&(&b.account, &b.region, &b.resource_type))
    });

    let mut resource_types: Vec<ResourceTypeInfo> = selected
        .iter()
        .map(|(resource_type, collector)| ResourceTypeInfo {
            resource_type: (*resource_type).to_owned(),
            scope: collector.scope(),
        })
        .collect();
    resource_types.push(ResourceTypeInfo {
        resource_type: ACCOUNT_RESOURCE_TYPE.to_owned(),
        scope: Scope::Global,
    });
    resource_types.sort_by(|a, b| a.resource_type.cmp(&b.resource_type));

    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        succeeded = sets.len(),
        failed = errors.len(),
        "finished the collection"
    );
    Ok(Inventory {
        accounts: vec![AccountInfo {
            account: identity.account_key.clone(),
            id: identity.account_id.clone(),
            provider: "aws".to_owned(),
        }],
        regions,
        resource_types,
        resource_sets: sets,
        collection_errors: errors,
        started_at,
        finished_at: SystemTime::now(),
    })
}

/// Runs the groups with bounded concurrency and routes each result.
async fn run_groups<F>(
    groups: Vec<Group>,
    identity: &Identity,
    context: &Arc<CollectContext>,
    concurrency: usize,
    cancel: &mut F,
) -> Result<(Vec<ResourceSet>, Vec<CollectionError>), ProviderError>
where
    F: Future<Output = ()> + Unpin,
{
    let semaphore = Arc::new(Semaphore::new(concurrency.clamp(1, Semaphore::MAX_PERMITS)));
    let mut tasks: JoinSet<GroupOutcome> = JoinSet::new();
    let mut task_infos: HashMap<tokio::task::Id, GroupInfo> = HashMap::new();
    let mut sets = Vec::new();
    let mut errors = Vec::new();

    for group in groups {
        let info = GroupInfo {
            account: identity.account_key.clone(),
            resource_type: group.resource_type,
            scope: group.scope,
            region: group.group_region.clone(),
        };
        // Acquire the permit in this loop, not in the task, so groups start in order.
        let permit = tokio::select! {
            biased;
            () = &mut *cancel => {
                tasks.shutdown().await;
                return Err(ProviderError::Interrupted);
            }
            permit = Arc::clone(&semaphore).acquire_owned() => permit,
        };
        let Ok(permit) = permit else {
            record(
                GroupOutcome::Failed(CollectionError {
                    account: info.account,
                    region: info.region,
                    resource_type: info.resource_type.to_owned(),
                    kind: CollectionErrorKind::Service,
                    message: "failed to acquire a concurrency permit".to_owned(),
                }),
                &mut sets,
                &mut errors,
            );
            continue;
        };
        let context = Arc::clone(context);
        let task_info = info.clone();
        let handle = tasks.spawn(async move {
            let _permit = permit;
            tracing::debug!(
                resource_type = task_info.resource_type,
                region = %task_info.region,
                "group started"
            );
            let result = group.collector.collect(&context, &group.api_region).await;
            let outcome = route_result(&task_info, result);
            if let GroupOutcome::Collected(set) = &outcome {
                tracing::debug!(
                    resource_type = task_info.resource_type,
                    region = %task_info.region,
                    resources = set.resources.len(),
                    "group finished"
                );
            }
            outcome
        });
        task_infos.insert(handle.id(), info);
    }

    loop {
        let joined = tokio::select! {
            biased;
            () = &mut *cancel => {
                tasks.shutdown().await;
                return Err(ProviderError::Interrupted);
            }
            joined = tasks.join_next_with_id() => joined,
        };
        match joined {
            None => break,
            Some(Ok((_, outcome))) => record(outcome, &mut sets, &mut errors),
            Some(Err(join_error)) => {
                let Some(info) = task_infos.get(&join_error.id()) else {
                    continue;
                };
                let message = if join_error.is_panic() {
                    "collector panicked"
                } else {
                    "collector task was cancelled"
                };
                record(
                    GroupOutcome::Failed(CollectionError {
                        account: info.account.clone(),
                        region: info.region.clone(),
                        resource_type: info.resource_type.to_owned(),
                        kind: CollectionErrorKind::Service,
                        message: message.to_owned(),
                    }),
                    &mut sets,
                    &mut errors,
                );
            }
        }
    }
    Ok((sets, errors))
}

/// Stores an outcome in the matching list and logs failures.
fn record(outcome: GroupOutcome, sets: &mut Vec<ResourceSet>, errors: &mut Vec<CollectionError>) {
    match outcome {
        GroupOutcome::Collected(set) => sets.push(set),
        GroupOutcome::Failed(error) => {
            tracing::warn!(
                account = %error.account,
                region = %error.region,
                resource_type = %error.resource_type,
                kind = %error.kind,
                "collection failed: {}",
                error.message
            );
            errors.push(error);
        }
    }
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use aws_sdk_ec2::operation::describe_regions::DescribeRegionsOutput;
    use aws_sdk_ec2::types::Region as Ec2Region;
    use aws_sdk_sts::operation::get_caller_identity::GetCallerIdentityOutput;
    use aws_smithy_mocks::{Rule, mock};

    use super::*;
    use crate::collector::CollectFuture;
    use crate::support::clients::testing::mock_factory;

    const ACCOUNT_KEY: &str = "aws-123456789012";

    #[derive(Default)]
    struct Probe {
        running: AtomicUsize,
        max_running: AtomicUsize,
        started: Mutex<Vec<String>>,
    }

    struct RunGuard(Arc<Probe>);

    impl RunGuard {
        fn enter(probe: &Arc<Probe>, label: String) -> Self {
            let now = probe.running.fetch_add(1, Ordering::SeqCst) + 1;
            probe.max_running.fetch_max(now, Ordering::SeqCst);
            probe.started.lock().expect("lock").push(label);
            RunGuard(Arc::clone(probe))
        }
    }

    impl Drop for RunGuard {
        fn drop(&mut self) {
            self.0.running.fetch_sub(1, Ordering::SeqCst);
        }
    }

    enum Behavior {
        /// Returns the given number of resources with ids `r0`, `r1`, ... in reverse order.
        Resources(usize),
        /// Returns the given resources as they are.
        Raw(fn(&str, &str) -> Vec<Resource>),
        Fail(CollectError),
        Panic,
        Never,
    }

    struct Fake {
        resource_type: &'static str,
        scope: Scope,
        delay: Duration,
        behavior: Behavior,
        probe: Arc<Probe>,
        seen_regions: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(resource_type: &'static str, scope: Scope, probe: &Arc<Probe>) -> Self {
            Fake {
                resource_type,
                scope,
                delay: Duration::ZERO,
                behavior: Behavior::Resources(1),
                probe: Arc::clone(probe),
                seen_regions: Mutex::new(Vec::new()),
            }
        }
        fn delay(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
        fn behavior(mut self, behavior: Behavior) -> Self {
            self.behavior = behavior;
            self
        }
        fn boxed(self) -> Box<dyn Collector> {
            Box::new(self)
        }
    }

    fn resource(resource_type: &str, account: &str, region: &str, id: &str) -> Resource {
        Resource {
            id: id.to_owned(),
            resource_type: resource_type.to_owned(),
            account: account.to_owned(),
            region: region.to_owned(),
            name: None,
            tags: Default::default(),
            configuration: Default::default(),
        }
    }

    impl Collector for Fake {
        fn resource_type(&self) -> &'static str {
            self.resource_type
        }
        fn scope(&self) -> Scope {
            self.scope
        }
        fn required_permissions(&self) -> &'static [&'static str] {
            &["sts:GetCallerIdentity"]
        }
        fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
            Box::pin(async move {
                let _guard =
                    RunGuard::enter(&self.probe, format!("{}@{region}", self.resource_type));
                self.seen_regions
                    .lock()
                    .expect("lock")
                    .push(region.to_owned());
                assert_eq!(ctx.account_key(), ACCOUNT_KEY);
                assert_eq!(ctx.account_id(), "123456789012");
                if !self.delay.is_zero() {
                    tokio::time::sleep(self.delay).await;
                }
                match &self.behavior {
                    Behavior::Resources(count) => {
                        let resource_region = match self.scope {
                            Scope::Global => "global",
                            Scope::Regional => region,
                        };
                        Ok((0..*count)
                            .rev()
                            .map(|i| {
                                resource(
                                    self.resource_type,
                                    ACCOUNT_KEY,
                                    resource_region,
                                    &format!("r{i}"),
                                )
                            })
                            .collect())
                    }
                    Behavior::Raw(make) => Ok(make(self.resource_type, region)),
                    Behavior::Fail(err) => Err(err.clone()),
                    Behavior::Panic => panic!("fake collector panic"),
                    Behavior::Never => {
                        std::future::pending::<()>().await;
                        Ok(Vec::new())
                    }
                }
            })
        }
    }

    fn identity_rule() -> Rule {
        mock!(aws_sdk_sts::Client::get_caller_identity).then_output(|| {
            GetCallerIdentityOutput::builder()
                .account("123456789012")
                .arn("arn:aws:iam::123456789012:user/example")
                .user_id("AIDAEXAMPLE")
                .build()
        })
    }

    fn regions_rule(names: &'static [&'static str]) -> Rule {
        mock!(aws_sdk_ec2::Client::describe_regions).then_output(move || {
            DescribeRegionsOutput::builder()
                .set_regions(Some(
                    names
                        .iter()
                        .map(|n| Ec2Region::builder().region_name(*n).build())
                        .collect(),
                ))
                .build()
        })
    }

    fn params(types: &[&str], regions: Option<&[&str]>, concurrency: usize) -> RunParams {
        RunParams {
            resource_types: types.iter().map(|t| (*t).to_owned()).collect(),
            regions: regions.map(|r| r.iter().map(|s| (*s).to_owned()).collect()),
            concurrency,
        }
    }

    async fn run_with(
        rules: &[Rule],
        params: RunParams,
        collectors: Vec<Box<dyn Collector>>,
    ) -> Result<Inventory, ProviderError> {
        let mut cancel = Box::pin(std::future::pending::<()>());
        run(
            params,
            mock_factory(rules),
            "us-east-1",
            collectors,
            &mut cancel,
        )
        .await
    }

    fn standard_rules() -> Vec<Rule> {
        vec![
            identity_rule(),
            regions_rule(&["us-west-2", "ap-northeast-1", "eu-west-1"]),
        ]
    }

    #[tokio::test]
    async fn expands_global_groups_first_then_regions_and_types_ascending() {
        let probe = Arc::new(Probe::default());
        let collectors = [
            Fake::new("aws.s3.bucket", Scope::Global, &probe),
            Fake::new("aws.ec2.vpc", Scope::Regional, &probe),
            Fake::new("aws.iam.user", Scope::Global, &probe),
            Fake::new("aws.cloudtrail.trail", Scope::Regional, &probe),
        ]
        .into_iter()
        .map(Fake::boxed)
        .collect();
        let map = collector_map(collectors);
        let groups = expand_groups(&map, &["a-east-1".to_owned(), "b-west-2".to_owned()]);
        let order: Vec<String> = groups
            .iter()
            .map(|g| format!("{}@{}/{}", g.resource_type, g.group_region, g.api_region))
            .collect();
        assert_eq!(
            order,
            [
                "aws.iam.user@global/us-east-1",
                "aws.s3.bucket@global/us-east-1",
                "aws.cloudtrail.trail@a-east-1/a-east-1",
                "aws.ec2.vpc@a-east-1/a-east-1",
                "aws.cloudtrail.trail@b-west-2/b-west-2",
                "aws.ec2.vpc@b-west-2/b-west-2",
            ]
        );
    }

    #[tokio::test]
    async fn collects_all_groups_and_orders_the_inventory() {
        let probe = Arc::new(Probe::default());
        let global =
            Fake::new("aws.iam.user", Scope::Global, &probe).behavior(Behavior::Resources(3));
        let regional =
            Fake::new("aws.ec2.vpc", Scope::Regional, &probe).behavior(Behavior::Resources(0));
        let inventory = run_with(
            &standard_rules(),
            params(&["aws.ec2.vpc", "aws.iam.user"], None, 4),
            vec![regional.boxed(), global.boxed()],
        )
        .await
        .expect("collects");

        assert_eq!(
            inventory.regions,
            ["ap-northeast-1", "eu-west-1", "us-west-2"]
        );
        assert_eq!(
            inventory
                .resource_types
                .iter()
                .map(|t| (t.resource_type.as_str(), t.scope))
                .collect::<Vec<_>>(),
            [
                ("aws.account.account", Scope::Global),
                ("aws.ec2.vpc", Scope::Regional),
                ("aws.iam.user", Scope::Global),
            ]
        );
        assert_eq!(inventory.accounts.len(), 1);
        assert_eq!(inventory.accounts[0].account, ACCOUNT_KEY);
        assert_eq!(inventory.accounts[0].id, "123456789012");
        assert_eq!(inventory.accounts[0].provider, "aws");
        assert!(inventory.collection_errors.is_empty());

        let keys: Vec<(&str, &str)> = inventory
            .resource_sets
            .iter()
            .map(|s| (s.region.as_str(), s.resource_type.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("ap-northeast-1", "aws.ec2.vpc"),
                ("eu-west-1", "aws.ec2.vpc"),
                ("global", "aws.account.account"),
                ("global", "aws.iam.user"),
                ("us-west-2", "aws.ec2.vpc"),
            ]
        );
        let users = &inventory.resource_sets[3];
        let ids: Vec<&str> = users.resources.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["r0", "r1", "r2"]);
        // A group with 0 resources still produces a ResourceSet.
        assert!(inventory.resource_sets[0].resources.is_empty());
    }

    #[tokio::test]
    async fn global_collectors_get_the_default_region_and_regional_ones_their_own() {
        let probe = Arc::new(Probe::default());
        let global = Arc::new(Fake::new("aws.iam.user", Scope::Global, &probe));
        let regional = Arc::new(Fake::new("aws.ec2.vpc", Scope::Regional, &probe));

        struct Shared(Arc<Fake>);
        impl Collector for Shared {
            fn resource_type(&self) -> &'static str {
                self.0.resource_type()
            }
            fn scope(&self) -> Scope {
                self.0.scope()
            }
            fn required_permissions(&self) -> &'static [&'static str] {
                self.0.required_permissions()
            }
            fn collect<'a>(
                &'a self,
                ctx: &'a CollectContext,
                region: &'a str,
            ) -> CollectFuture<'a> {
                self.0.collect(ctx, region)
            }
        }

        run_with(
            &[identity_rule()],
            params(
                &["aws.ec2.vpc", "aws.iam.user"],
                Some(&["eu-west-1", "ap-northeast-1"]),
                4,
            ),
            vec![
                Box::new(Shared(global.clone())),
                Box::new(Shared(regional.clone())),
            ],
        )
        .await
        .expect("collects");
        assert_eq!(*global.seen_regions.lock().expect("lock"), ["us-east-1"]);
        let mut seen = regional.seen_regions.lock().expect("lock").clone();
        seen.sort();
        assert_eq!(seen, ["ap-northeast-1", "eu-west-1"]);
    }

    #[tokio::test]
    async fn regions_given_means_describe_regions_is_not_called() {
        let regions = regions_rule(&["us-west-2"]);
        let inventory = run_with(
            &[identity_rule(), regions.clone()],
            params(&[], Some(&["us-west-2", "ap-northeast-1"]), 2),
            Vec::new(),
        )
        .await
        .expect("collects");
        assert_eq!(regions.num_calls(), 0);
        assert_eq!(inventory.regions, ["ap-northeast-1", "us-west-2"]);
    }

    #[tokio::test]
    async fn account_resource_is_always_present() {
        let inventory = run_with(&standard_rules(), params(&[], None, 2), Vec::new())
            .await
            .expect("collects");
        assert_eq!(inventory.accounts.len(), 1);
        assert_eq!(inventory.resource_types.len(), 1);
        assert_eq!(
            inventory.resource_types[0].resource_type,
            "aws.account.account"
        );
        assert_eq!(inventory.resource_types[0].scope, Scope::Global);
        assert_eq!(inventory.resource_sets.len(), 1);
        let set = &inventory.resource_sets[0];
        assert_eq!(set.region, "global");
        assert_eq!(set.resource_type, "aws.account.account");
        assert_eq!(set.resources.len(), 1);
        assert!(inventory.collection_errors.is_empty());
    }

    #[tokio::test]
    async fn a_failed_group_is_recorded_and_the_others_continue() {
        let probe = Arc::new(Probe::default());
        let failing = Fake::new("aws.iam.user", Scope::Global, &probe).behavior(Behavior::Fail(
            CollectError::new(
                CollectionErrorKind::AccessDenied,
                "access denied when calling ListUsers (AccessDenied)",
            ),
        ));
        let ok = Fake::new("aws.ec2.vpc", Scope::Regional, &probe);
        let inventory = run_with(
            &standard_rules(),
            params(&["aws.iam.user", "aws.ec2.vpc"], Some(&["us-west-2"]), 4),
            vec![failing.boxed(), ok.boxed()],
        )
        .await
        .expect("collects");
        assert_eq!(inventory.collection_errors.len(), 1);
        let error = &inventory.collection_errors[0];
        assert_eq!(error.account, ACCOUNT_KEY);
        assert_eq!(error.region, "global");
        assert_eq!(error.resource_type, "aws.iam.user");
        assert_eq!(error.kind, CollectionErrorKind::AccessDenied);
        assert!(error.message.contains("ListUsers"));
        assert_exactly_one_outcome_per_group(&inventory, 3);
    }

    /// Each targeted group appears in exactly one of resource_sets / collection_errors.
    fn assert_exactly_one_outcome_per_group(inventory: &Inventory, expected_groups: usize) {
        let mut seen: BTreeMap<(String, String, String), usize> = BTreeMap::new();
        for set in &inventory.resource_sets {
            *seen
                .entry((
                    set.account.clone(),
                    set.region.clone(),
                    set.resource_type.clone(),
                ))
                .or_default() += 1;
        }
        for error in &inventory.collection_errors {
            *seen
                .entry((
                    error.account.clone(),
                    error.region.clone(),
                    error.resource_type.clone(),
                ))
                .or_default() += 1;
        }
        assert_eq!(seen.len(), expected_groups, "{seen:?}");
        assert!(seen.values().all(|count| *count == 1), "{seen:?}");
    }

    #[tokio::test]
    async fn invalid_collector_output_becomes_invalid_response() {
        let probe = Arc::new(Probe::default());
        let wrong_type =
            Fake::new("aws.iam.user", Scope::Global, &probe).behavior(Behavior::Raw(|_, _| {
                vec![resource("aws.iam.role", ACCOUNT_KEY, "global", "a")]
            }));
        let wrong_account =
            Fake::new("aws.iam.role", Scope::Global, &probe).behavior(Behavior::Raw(|t, _| {
                vec![resource(t, "aws-999999999999", "global", "a")]
            }));
        let duplicate =
            Fake::new("aws.iam.group", Scope::Global, &probe).behavior(Behavior::Raw(|t, _| {
                vec![
                    resource(t, ACCOUNT_KEY, "global", "a"),
                    resource(t, ACCOUNT_KEY, "global", "a"),
                ]
            }));
        let wrong_region =
            Fake::new("aws.ec2.vpc", Scope::Regional, &probe).behavior(Behavior::Raw(|t, _| {
                vec![resource(t, ACCOUNT_KEY, "eu-west-1", "a")]
            }));
        let inventory = run_with(
            &[identity_rule()],
            params(
                &[
                    "aws.iam.user",
                    "aws.iam.role",
                    "aws.iam.group",
                    "aws.ec2.vpc",
                ],
                Some(&["us-west-2"]),
                4,
            ),
            vec![
                wrong_type.boxed(),
                wrong_account.boxed(),
                duplicate.boxed(),
                wrong_region.boxed(),
            ],
        )
        .await
        .expect("collects");
        assert_eq!(
            inventory.collection_errors.len(),
            4,
            "{:?}",
            inventory.collection_errors
        );
        assert!(
            inventory
                .collection_errors
                .iter()
                .all(|e| e.kind == CollectionErrorKind::InvalidResponse)
        );
        assert_eq!(
            inventory.resource_sets.len(),
            1,
            "only the account set remains"
        );
        assert_exactly_one_outcome_per_group(&inventory, 5);
    }

    #[tokio::test]
    async fn global_types_may_hold_resources_of_other_regions() {
        let probe = Arc::new(Probe::default());
        let bucket =
            Fake::new("aws.s3.bucket", Scope::Global, &probe).behavior(Behavior::Raw(|t, _| {
                vec![resource(t, ACCOUNT_KEY, "eu-west-1", "bucket-a")]
            }));
        let inventory = run_with(
            &[identity_rule()],
            params(&["aws.s3.bucket"], Some(&["us-west-2"]), 2),
            vec![bucket.boxed()],
        )
        .await
        .expect("collects");
        assert!(inventory.collection_errors.is_empty());
        assert_eq!(inventory.resource_sets.len(), 2);
    }

    #[tokio::test]
    async fn a_panicking_collector_fails_only_its_group() {
        let probe = Arc::new(Probe::default());
        let panicking = Fake::new("aws.iam.user", Scope::Global, &probe).behavior(Behavior::Panic);
        let ok = Fake::new("aws.iam.role", Scope::Global, &probe);
        let inventory = run_with(
            &[identity_rule()],
            params(&["aws.iam.user", "aws.iam.role"], Some(&["us-west-2"]), 4),
            vec![panicking.boxed(), ok.boxed()],
        )
        .await
        .expect("collects");
        assert_eq!(inventory.collection_errors.len(), 1);
        let error = &inventory.collection_errors[0];
        assert_eq!(error.resource_type, "aws.iam.user");
        assert_eq!(error.kind, CollectionErrorKind::Service);
        assert_eq!(error.message, "collector panicked");
        assert_exactly_one_outcome_per_group(&inventory, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrency_limit_is_respected() {
        let probe = Arc::new(Probe::default());
        let types = [
            "aws.ec2.a",
            "aws.ec2.b",
            "aws.ec2.c",
            "aws.ec2.d",
            "aws.ec2.e",
            "aws.ec2.f",
        ];
        let collectors = types
            .iter()
            .map(|t| {
                Fake::new(t, Scope::Regional, &probe)
                    .delay(Duration::from_millis(50))
                    .boxed()
            })
            .collect();
        let inventory = run_with(
            &[identity_rule()],
            params(&types, Some(&["us-west-2"]), 2),
            collectors,
        )
        .await
        .expect("collects");
        assert_eq!(inventory.resource_sets.len(), 7);
        assert_eq!(probe.max_running.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn groups_start_in_the_expansion_order() {
        let probe = Arc::new(Probe::default());
        let collectors = [
            Fake::new("aws.s3.bucket", Scope::Global, &probe),
            Fake::new("aws.ec2.vpc", Scope::Regional, &probe),
            Fake::new("aws.iam.user", Scope::Global, &probe),
            Fake::new("aws.cloudtrail.trail", Scope::Regional, &probe),
        ]
        .into_iter()
        .map(Fake::boxed)
        .collect();
        run_with(
            &[identity_rule()],
            params(
                &[
                    "aws.s3.bucket",
                    "aws.ec2.vpc",
                    "aws.iam.user",
                    "aws.cloudtrail.trail",
                ],
                Some(&["us-west-2", "ap-northeast-1"]),
                1,
            ),
            collectors,
        )
        .await
        .expect("collects");
        let started = probe.started.lock().expect("lock").clone();
        assert_eq!(
            started,
            [
                "aws.iam.user@us-east-1",
                "aws.s3.bucket@us-east-1",
                "aws.cloudtrail.trail@ap-northeast-1",
                "aws.ec2.vpc@ap-northeast-1",
                "aws.cloudtrail.trail@us-west-2",
                "aws.ec2.vpc@us-west-2",
            ]
        );
    }

    #[tokio::test]
    async fn cancel_aborts_running_tasks_and_returns_interrupted() {
        let probe = Arc::new(Probe::default());
        let collectors = (0..3)
            .map(|i| {
                let name: &'static str = ["aws.ec2.a", "aws.ec2.b", "aws.ec2.c"][i];
                Fake::new(name, Scope::Regional, &probe)
                    .behavior(Behavior::Never)
                    .boxed()
            })
            .collect();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut cancel = Box::pin(async move {
            let _ = rx.await;
        });
        let probe_for_cancel = Arc::clone(&probe);
        let canceller = tokio::spawn(async move {
            // Bounded, so a regression fails the test instead of hanging it.
            let _ = tokio::time::timeout(Duration::from_secs(20), async {
                while probe_for_cancel.running.load(Ordering::SeqCst) < 2 {
                    tokio::task::yield_now().await;
                }
            })
            .await;
            let _ = tx.send(());
        });
        let result = run(
            params(
                &["aws.ec2.a", "aws.ec2.b", "aws.ec2.c"],
                Some(&["us-west-2"]),
                2,
            ),
            mock_factory(&[identity_rule()]),
            "us-east-1",
            collectors,
            &mut cancel,
        )
        .await;
        canceller.await.expect("canceller finishes");
        assert!(
            matches!(result, Err(ProviderError::Interrupted)),
            "{result:?}"
        );
        // Dropping the collector futures decremented the counter: the tasks were aborted.
        assert_eq!(probe.running.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancel_while_waiting_for_the_account_call_returns_interrupted() {
        let mut cancel = Box::pin(std::future::ready(()));
        let result = run(
            params(&[], Some(&["us-west-2"]), 1),
            mock_factory(&[identity_rule()]),
            "us-east-1",
            Vec::new(),
            &mut cancel,
        )
        .await;
        assert!(
            matches!(result, Err(ProviderError::Interrupted)),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn setup_failures_are_provider_errors() {
        let rule = mock!(aws_sdk_sts::Client::get_caller_identity).then_output(|| {
            GetCallerIdentityOutput::builder()
                .account("123456789012")
                .arn("arn:aws-cn:iam::123456789012:user/example")
                .build()
        });
        let result = run_with(&[rule], params(&[], Some(&["us-west-2"]), 1), Vec::new()).await;
        assert!(
            matches!(
                result,
                Err(ProviderError::Setup(
                    SetupError::UnsupportedPartition { .. }
                ))
            ),
            "{result:?}"
        );
    }

    use crate::error::SetupError;
}
