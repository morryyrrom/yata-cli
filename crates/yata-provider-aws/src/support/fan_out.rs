//! Bounded concurrent execution of per-resource tasks inside one collection group.

// Used by the service collectors that are added in later changes.
#![allow(dead_code)]

use std::future::Future;
use std::num::NonZeroUsize;

use tokio::task::JoinSet;
use yata_core::CollectionErrorKind;

use super::errors::CollectError;

/// Runs `task` for every item with at most `limit` tasks running at once and returns
/// the outputs in the order of `items`.
///
/// Tasks are spawned in the order of `items`. A task that panics fails the whole group
/// with a `service` error; the remaining tasks are aborted. Dropping the returned future
/// aborts all running tasks.
pub(crate) async fn fan_out<I, T, F, Fut>(
    limit: NonZeroUsize,
    items: Vec<I>,
    mut task: F,
) -> Result<Vec<T>, CollectError>
where
    I: Send + 'static,
    T: Send + 'static,
    F: FnMut(I) -> Fut,
    Fut: Future<Output = T> + Send + 'static,
{
    let total = items.len();
    let mut slots: Vec<Option<T>> = Vec::with_capacity(total);
    slots.resize_with(total, || None);
    let mut set: JoinSet<(usize, T)> = JoinSet::new();

    for (index, item) in items.into_iter().enumerate() {
        while set.len() >= limit.get() {
            join_one(&mut set, &mut slots).await?;
        }
        let fut = task(item);
        set.spawn(async move { (index, fut.await) });
    }
    while !set.is_empty() {
        join_one(&mut set, &mut slots).await?;
    }

    let mut outputs = Vec::with_capacity(total);
    for slot in slots {
        match slot {
            Some(output) => outputs.push(output),
            None => return Err(panic_error()),
        }
    }
    Ok(outputs)
}

/// Waits for one task to finish and stores its output in its slot.
async fn join_one<T: 'static>(
    set: &mut JoinSet<(usize, T)>,
    slots: &mut [Option<T>],
) -> Result<(), CollectError> {
    match set.join_next().await {
        Some(Ok((index, output))) => {
            if let Some(slot) = slots.get_mut(index) {
                *slot = Some(output);
            }
            Ok(())
        }
        Some(Err(err)) => {
            tracing::debug!(error = ?err, "fan-out task failed");
            Err(panic_error())
        }
        None => Ok(()),
    }
}

/// The error for a panicked (or lost) task.
fn panic_error() -> CollectError {
    CollectError::new(CollectionErrorKind::Service, "collector panicked")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn limit(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn never_exceeds_the_limit_and_keeps_input_order() {
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let items: Vec<u64> = (0..20).collect();
        let (r, p) = (running.clone(), peak.clone());
        let out = fan_out(limit(3), items, move |i| {
            let (r, p) = (r.clone(), p.clone());
            async move {
                let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(now, Ordering::SeqCst);
                // Later items finish earlier, so completion order differs from input order.
                tokio::time::sleep(Duration::from_millis(100 - i * 3)).await;
                r.fetch_sub(1, Ordering::SeqCst);
                i * 10
            }
        })
        .await
        .unwrap();
        assert_eq!(out, (0..20).map(|i| i * 10).collect::<Vec<u64>>());
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn empty_input_gives_empty_output() {
        let out = fan_out(limit(4), Vec::<u32>::new(), |i| async move { i })
            .await
            .unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn a_panicking_task_fails_the_group_with_a_service_error() {
        let result = fan_out(limit(2), vec![1u32, 2, 3], |i| async move {
            if i == 2 {
                panic!("boom");
            }
            i
        })
        .await;
        let err = result.unwrap_err();
        assert_eq!(err.kind(), CollectionErrorKind::Service);
        assert_eq!(err.message(), "collector panicked");
    }

    struct DropCounter(Arc<AtomicUsize>);

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_future_aborts_running_tasks() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let items: Vec<DropCounter> = (0..4).map(|_| DropCounter(dropped.clone())).collect();
        let fut = fan_out(limit(4), items, |guard| async move {
            let _guard = guard;
            tokio::time::sleep(Duration::from_secs(3600)).await;
        });
        // The timeout drops the fan_out future while all four tasks are sleeping.
        let timed_out = tokio::time::timeout(Duration::from_secs(1), fut).await;
        assert!(timed_out.is_err());
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(dropped.load(Ordering::SeqCst), 4);
    }
}
