//! TTL cache with in-flight request deduplication, mirroring
//! `@std/cache` `memoize` + `TtlCache`: concurrent lookups of the same key
//! share a single computation, entries expire after the TTL, and failures
//! (`None`) are cached like the JS `Promise<...|undefined>` results.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{watch, Mutex};

struct Entry {
    expires_at: Instant,
    /// watch value: `None` while the computation is in flight.
    state: Arc<EntryState>,
}

struct EntryState {
    value: watch::Sender<Option<Option<String>>>,
}

struct Inner {
    ttl: Duration,
    map: Mutex<Vec<(String, Entry)>>,
}

#[derive(Clone)]
pub struct TtlCache {
    inner: Arc<Inner>,
}

impl TtlCache {
    pub fn new(ttl: Duration) -> Self {
        let cache = TtlCache {
            inner: Arc::new(Inner {
                ttl,
                map: Mutex::new(Vec::new()),
            }),
        };
        // Periodic sweep so `entries` (and memory) stay honest between lookups.
        let purger = cache.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                purger.sweep().await;
            }
        });
        cache
    }

    async fn sweep(&self) {
        let now = Instant::now();
        let mut map = self.inner.map.lock().await;
        map.retain(|(_, e)| e.expires_at > now);
    }

    /// Number of live entries (expired ones purged first).
    pub async fn len(&self) -> usize {
        self.sweep().await;
        self.inner.map.lock().await.len()
    }

    /// Return the cached value for `key`, computing it with `compute` on a
    /// miss. Concurrent callers await the same in-flight computation.
    pub async fn get_or_compute<F, Fut>(&self, key: String, compute: F) -> Option<String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<String>> + Send + 'static,
    {
        loop {
            let now = Instant::now();
            let mut map = self.inner.map.lock().await;
            map.retain(|(_, e)| e.expires_at > now);

            if let Some((_, entry)) = map.iter().find(|(k, _)| *k == key) {
                let mut rx = entry.state.value.subscribe();
                // A previous computation may have finished between the lock
                // and the subscribe; re-check before waiting.
                if let Some(Some(v)) = rx.borrow_and_update().as_ref() {
                    return Some(v.clone());
                }
                drop(map);
                match wait_for_value(&mut rx).await {
                    // Published result (including cached failures).
                    Some(result) => return result,
                    // Sender vanished (entry swept while in flight): retry.
                    None => continue,
                }
            }

            // Miss: insert our entry and compute.
            let (tx, _rx) = watch::channel(None);
            let state = Arc::new(EntryState { value: tx });
            map.push((
                key.clone(),
                Entry {
                    expires_at: now + self.inner.ttl,
                    state: state.clone(),
                },
            ));
            let future = compute();
            drop(map);
            let task_state = state.clone();
            let task = tokio::spawn(async move {
                let result = future.await;
                let _ = task_state.value.send(Some(result.clone()));
                result
            });
            return task.await.unwrap_or_default();
        }
    }
}

/// Await a computation result published through the watch channel.
/// Returns `None` when the sender is dropped (entry expired mid-flight).
async fn wait_for_value(
    rx: &mut watch::Receiver<Option<Option<String>>>,
) -> Option<Option<String>> {
    loop {
        if let Some(v) = rx.borrow().clone() {
            return Some(v);
        }
        if rx.changed().await.is_err() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn caches_value_and_dedups_inflight() {
        let cache = TtlCache::new(Duration::from_secs(60));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let c1 = cache.clone();
        let c2 = cache.clone();

        let t1 = tokio::spawn(async move {
            c1.get_or_compute("k".to_string(), || async move {
                calls2.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                Some("v".to_string())
            })
            .await
        });
        let calls3 = calls.clone();
        let t2 = tokio::spawn(async move {
            c2.get_or_compute("k".to_string(), || async move {
                calls3.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                Some("v".to_string())
            })
            .await
        });

        assert_eq!(t1.await.unwrap(), Some("v".to_string()));
        assert_eq!(t2.await.unwrap(), Some("v".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "compute must run once");
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn expires_after_ttl() {
        let cache = TtlCache::new(Duration::from_millis(50));
        let c = cache.clone();
        let first = c
            .get_or_compute("k".to_string(), || async { Some("a".to_string()) })
            .await;
        assert_eq!(first, Some("a".to_string()));
        tokio::time::sleep(Duration::from_millis(80)).await;
        let second = c
            .get_or_compute("k".to_string(), || async { Some("b".to_string()) })
            .await;
        assert_eq!(second, Some("b".to_string()));
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn caches_failures_as_none() {
        let cache = TtlCache::new(Duration::from_secs(60));
        let c = cache.clone();
        assert_eq!(
            c.get_or_compute("k".to_string(), || async { None }).await,
            None
        );
        // Second call must NOT recompute: the failure is memoized (JS caches
        // resolved-undefined the same way).
        assert_eq!(
            c.get_or_compute("k".to_string(), || async { Some("x".to_string()) })
                .await,
            None
        );
        assert_eq!(cache.len().await, 1);
    }
}
