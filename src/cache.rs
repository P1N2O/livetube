//! TTL cache with in-flight request deduplication and an optional persistent
//! backend, mirroring `@std/cache` `memoize` + `TtlCache` semantics:
//! concurrent lookups of the same key share a single computation, and
//! failures (`None`) are cached like the JS `Promise<...|undefined>` results.
//!
//! Each entry carries its own lifetime, decided by the resolver:
//!   - successful lookups: `MEMOIZATION_TTL`, capped by the stream URL's
//!     own `expire` token when present;
//!   - failed lookups: `CACHE_NEGATIVE_TTL` (short, so dead streams recover).
//!
//! Persistence (L2) is optional:
//!   - `CACHE_REDIS_URL` set  → shared Redis cache (SETEX, auto-expiry);
//!   - else `CACHE_DIR` set   → redb file under `$CACHE_DIR/redb/cache.redb`;
//!   - else                   → in-memory only (the original behavior).
//!
//! Backend failures degrade to memory; they never fail a request.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{watch, Mutex};

use redb::ReadableDatabase;
use redb::ReadableTable;

/// Computed value plus its cache lifetime (chosen by the resolver).
pub type CacheResult = (Option<String>, Duration);

const REDB_TABLE: redb::TableDefinition<&str, (u64, &str)> = redb::TableDefinition::new("cache");
const REDIS_PREFIX: &str = "livetube:";

struct EntryState {
    tx: watch::Sender<Option<(Option<String>, Instant)>>,
    rx: watch::Receiver<Option<(Option<String>, Instant)>>,
}

struct Inner {
    map: Mutex<Vec<(String, Arc<EntryState>)>>,
    backend: Option<Backend>,
}

impl Inner {
    async fn sweep(&self) {
        let now = Instant::now();
        let mut map = self.map.lock().await;
        map.retain(|(_, state)| match state.rx.borrow().clone() {
            Some((_, exp)) => exp > now,
            None => true, // in-flight: keep
        });
        if let Some(backend) = &self.backend {
            backend.sweep().await;
        }
    }
}

/// Persistent cache backend. Values are stored with an absolute unix-ms
/// expiry; an empty string encodes a cached failure (`None`).
#[derive(Clone)]
pub enum Backend {
    Redb {
        db: Arc<redb::Database>,
    },
    Redis {
        client: redis::Client,
        conn: Arc<Mutex<Option<redis::aio::ConnectionManager>>>,
    },
}

impl Backend {
    /// Open the redb database at `$cache_dir/redb/cache.redb`.
    /// Returns `None` on any error — persistence is best-effort.
    pub fn redb(cache_dir: &Path) -> Option<Backend> {
        let dir = cache_dir.join("redb");
        std::fs::create_dir_all(&dir).ok()?;
        let db = redb::Database::create(dir.join("cache.redb")).ok()?;
        Some(Backend::Redb { db: Arc::new(db) })
    }

    /// Redis backend; the connection is established lazily on first use so
    /// startup never blocks on the network.
    pub fn redis(client: redis::Client) -> Backend {
        Backend::Redis {
            client,
            conn: Arc::new(Mutex::new(None)),
        }
    }

    /// Borrow a live connection, connecting on first use (and reconnecting
    /// via ConnectionManager afterwards).
    async fn redis_conn(&self) -> Option<redis::aio::ConnectionManager> {
        let Backend::Redis { client, conn } = self else {
            return None;
        };
        let mut guard = conn.lock().await;
        if guard.is_none() {
            match redis::aio::ConnectionManager::new(client.clone()).await {
                Ok(manager) => *guard = Some(manager),
                Err(e) => {
                    eprintln!("⚠️ Redis cache connection failed ({e}); falling back to memory for this request");
                    return None;
                }
            }
        }
        guard.clone()
    }

    async fn get(&self, key: &str) -> Option<(Option<String>, Instant)> {
        match self {
            Backend::Redb { db } => {
                let read = db.begin_read().ok()?;
                let table = read.open_table(REDB_TABLE).ok()?;
                let row = table.get(key).ok()??;
                let (exp_ms, value) = row.value();
                Some((decode(value), expiry_instant(exp_ms)?))
            }
            Backend::Redis { .. } => {
                let conn = self.redis_conn().await?;
                let raw: Option<String> = redis::cmd("GET")
                    .arg(format!("{REDIS_PREFIX}{key}"))
                    .query_async(&mut conn.clone())
                    .await
                    .ok()?;
                let (exp_ms, value): (u64, String) = serde_json::from_str(&raw?).ok()?;
                Some((decode(&value), expiry_instant(exp_ms)?))
            }
        }
    }

    async fn set(&self, key: &str, value: &Option<String>, expires_at: Instant) {
        let ttl = expires_at.saturating_duration_since(Instant::now());
        if ttl.is_zero() {
            return;
        }
        let exp_ms = unix_ms() + ttl.as_millis() as u64;
        match self {
            Backend::Redb { db } => {
                let val = value.clone().unwrap_or_default();
                if let Ok(write) = db.begin_write() {
                    if let Ok(mut table) = write.open_table(REDB_TABLE) {
                        let _ = table.insert(key, (exp_ms, val.as_str()));
                    }
                    let _ = write.commit();
                }
            }
            Backend::Redis { .. } => {
                let Some(conn) = self.redis_conn().await else {
                    return;
                };
                let json = serde_json::to_string(&(exp_ms, value.clone().unwrap_or_default()))
                    .unwrap_or_default();
                let secs = ttl.as_secs() as i64 + u64::from(ttl.subsec_millis() > 0) as i64;
                let _: Result<(), _> = redis::cmd("SETEX")
                    .arg(format!("{REDIS_PREFIX}{key}"))
                    .arg(secs)
                    .arg(json)
                    .query_async(&mut conn.clone())
                    .await;
            }
        }
    }

    /// Drop expired rows (redb only; Redis expires via SETEX).
    async fn sweep(&self) {
        if let Backend::Redb { db } = self {
            let now = unix_ms();
            if let Ok(write) = db.begin_write() {
                if let Ok(mut table) = write.open_table(REDB_TABLE) {
                    let expired: Vec<String> = table
                        .iter()
                        .ok()
                        .into_iter()
                        .flatten()
                        .filter_map(|item| {
                            let (key, value) = item.ok()?;
                            (value.value().0 < now).then(|| key.value().to_string())
                        })
                        .collect();
                    for key in expired {
                        let _ = table.remove(key.as_str());
                    }
                }
                let _ = write.commit();
            }
        }
    }
}

fn decode(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn expiry_instant(exp_ms: u64) -> Option<Instant> {
    let ttl = Duration::from_millis(exp_ms.saturating_sub(unix_ms()));
    Instant::now()
        .checked_add(ttl)
        .filter(|exp| *exp > Instant::now())
}

#[derive(Clone)]
pub struct TtlCache {
    inner: Arc<Inner>,
}

impl TtlCache {
    pub fn new(backend: Option<Backend>) -> Self {
        let cache = TtlCache {
            inner: Arc::new(Inner {
                map: Mutex::new(Vec::new()),
                backend,
            }),
        };
        // Periodic sweep so `entries` (and memory/disk) stay honest between
        // lookups. Holds only a weak reference: the task exits when the cache
        // is dropped (releasing any backend file lock).
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let weak = Arc::downgrade(&cache.inner);
            handle.spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    let Some(cache) = weak.upgrade() else { return };
                    cache.sweep().await;
                }
            });
        }
        cache
    }

    async fn sweep(&self) {
        self.inner.sweep().await;
    }

    /// Number of live entries (expired ones purged first).
    pub async fn len(&self) -> usize {
        self.sweep().await;
        self.inner.map.lock().await.len()
    }

    /// Return the cached value for `key`, computing it with `compute` on a
    /// miss. Concurrent callers await the same in-flight computation.
    /// `compute` returns the value and the lifetime to cache it for.
    pub async fn get_or_compute<F, Fut>(&self, key: String, compute: F) -> Option<String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = CacheResult> + Send + 'static,
    {
        'outer: loop {
            let now = Instant::now();
            let mut map = self.inner.map.lock().await;
            map.retain(|(_, state)| match state.rx.borrow().clone() {
                Some((_, exp)) => exp > now,
                None => true,
            });

            if let Some((_, state)) = map.iter().find(|(k, _)| *k == key) {
                let mut rx = state.tx.subscribe();
                let current = rx.borrow_and_update().clone();
                match current {
                    Some((value, exp)) if exp > now => return value,
                    Some(_) => {
                        // Stale entry: drop it and recompute.
                        drop(map);
                        continue 'outer;
                    }
                    None => {
                        // In-flight computation: wait for it without the lock.
                        drop(map);
                        loop {
                            let current = rx.borrow().clone();
                            match current {
                                Some((value, exp)) if exp > Instant::now() => return value,
                                Some(_) | None => {}
                            }
                            if rx.changed().await.is_err() {
                                break; // sender dropped (recomputed elsewhere)
                            }
                        }
                        continue 'outer;
                    }
                }
            }

            // Memory miss: try the persistent backend.
            if let Some(backend) = &self.inner.backend {
                if let Some((value, exp)) = backend.get(&key).await {
                    let (tx, rx) = watch::channel(Some((value.clone(), exp)));
                    map.push((key.clone(), Arc::new(EntryState { tx, rx })));
                    drop(map);
                    return value;
                }
            }

            // Full miss: compute, publish, persist.
            let (tx, rx) = watch::channel(None);
            let state = Arc::new(EntryState { tx, rx });
            map.push((key.clone(), state.clone()));
            drop(map);

            let future = compute();
            let publish = state.clone();
            let task = tokio::spawn(async move {
                let (value, ttl) = future.await;
                let exp = Instant::now() + ttl;
                let _ = publish.tx.send(Some((value.clone(), exp)));
                (value, exp)
            });
            let (value, exp) = match task.await {
                Ok(result) => result,
                Err(_) => (None, Instant::now()),
            };
            if let Some(backend) = &self.inner.backend {
                backend.set(&key, &value, exp).await;
            }
            return value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn memory_cache() -> TtlCache {
        TtlCache::new(None)
    }

    #[tokio::test]
    async fn caches_value_and_dedups_inflight() {
        let cache = memory_cache();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let c1 = cache.clone();
        let c2 = cache.clone();

        let t1 = tokio::spawn(async move {
            c1.get_or_compute("k".to_string(), || async move {
                calls2.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                (Some("v".to_string()), Duration::from_secs(60))
            })
            .await
        });
        let calls3 = calls.clone();
        let t2 = tokio::spawn(async move {
            c2.get_or_compute("k".to_string(), || async move {
                calls3.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                (Some("v".to_string()), Duration::from_secs(60))
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
        let cache = memory_cache();
        let c = cache.clone();
        let first = c
            .get_or_compute("k".to_string(), || async {
                (Some("a".to_string()), Duration::from_millis(50))
            })
            .await;
        assert_eq!(first, Some("a".to_string()));
        tokio::time::sleep(Duration::from_millis(80)).await;
        let second = c
            .get_or_compute("k".to_string(), || async {
                (Some("b".to_string()), Duration::from_secs(60))
            })
            .await;
        assert_eq!(second, Some("b".to_string()));
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn caches_failures_with_short_ttl() {
        let cache = memory_cache();
        let c = cache.clone();
        // Failure cached for 40ms.
        assert_eq!(
            c.get_or_compute("k".to_string(), || async {
                (None, Duration::from_millis(40))
            })
            .await,
            None
        );
        // Still cached (no recompute).
        assert_eq!(
            c.get_or_compute("k".to_string(), || async {
                (Some("x".to_string()), Duration::from_secs(60))
            })
            .await,
            None
        );
        // After the short TTL the failure expires and the compute runs again.
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            c.get_or_compute("k".to_string(), || async {
                (Some("y".to_string()), Duration::from_secs(60))
            })
            .await,
            Some("y".to_string())
        );
    }

    #[tokio::test]
    async fn redb_persists_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("redb");
        std::fs::create_dir_all(&db_path).unwrap();

        // "First process": compute and persist.
        {
            let backend = Backend::redb(dir.path()).expect("redb opens");
            let cache = TtlCache::new(Some(backend));
            let calls = Arc::new(AtomicUsize::new(0));
            let calls2 = calls.clone();
            let value = cache
                .get_or_compute("persist:1".to_string(), move || async move {
                    calls2.fetch_add(1, Ordering::SeqCst);
                    (Some("stored-url".to_string()), Duration::from_secs(3600))
                })
                .await;
            assert_eq!(value, Some("stored-url".to_string()));
        }

        // "Second process": same db, value must come from disk (no compute).
        {
            let backend = Backend::redb(dir.path()).expect("redb reopens");
            let cache = TtlCache::new(Some(backend));
            let calls = Arc::new(AtomicUsize::new(0));
            let calls2 = calls.clone();
            let value = cache
                .get_or_compute("persist:1".to_string(), move || async move {
                    calls2.fetch_add(1, Ordering::SeqCst);
                    (Some("recomputed".to_string()), Duration::from_secs(3600))
                })
                .await;
            assert_eq!(value, Some("stored-url".to_string()));
            assert_eq!(calls.load(Ordering::SeqCst), 0, "no recompute on hit");
        }
    }

    #[tokio::test]
    async fn redb_expired_rows_are_swept() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::redb(dir.path()).expect("redb opens");
        let cache = TtlCache::new(Some(backend));
        let c = cache.clone();
        let _ = c
            .get_or_compute("gone".to_string(), || async {
                (Some("v".to_string()), Duration::from_millis(30))
            })
            .await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        cache.sweep().await;

        // After the sweep the backend row is gone: recompute happens.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let value = c
            .get_or_compute("gone".to_string(), move || async move {
                calls2.fetch_add(1, Ordering::SeqCst);
                (Some("fresh".to_string()), Duration::from_secs(60))
            })
            .await;
        assert_eq!(value, Some("fresh".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
