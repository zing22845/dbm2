use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::Mutex;

/// Connection pool key — identifies a unique connection by instance + connection name.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct PoolKey {
    pub instance_name: String,
    pub connection_name: String,
}

impl PoolKey {
    pub fn new(instance_name: impl Into<String>, connection_name: impl Into<String>) -> Self {
        Self {
            instance_name: instance_name.into(),
            connection_name: connection_name.into(),
        }
    }
}

/// Thread-safe global connection pool manager.
///
/// Manages connection pools keyed by [`PoolKey`], allowing multiple tabs
/// to share the same underlying pool. Uses `Arc` for cheap sharing and
/// automatic reference-counted cleanup.
pub struct ConnectionPoolManager<T> {
    pools: Mutex<HashMap<PoolKey, Arc<T>>>,
}

impl<T> ConnectionPoolManager<T> {
    pub fn new() -> Self {
        Self {
            pools: Mutex::new(HashMap::new()),
        }
    }

    /// Get an existing pool or create a new one via `create_fn`.
    ///
    /// Thread-safe: multiple tabs can call this concurrently for the same key;
    /// only one pool will be created (the second caller gets the first pool).
    ///
    /// Uses a two-phase protocol to avoid holding the lock during the
    /// potentially-slow pool creation (network I/O).
    pub async fn get_or_create<F, Fut, E>(
        &self,
        key: PoolKey,
        create_fn: F,
    ) -> std::result::Result<Arc<T>, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = std::result::Result<T, E>>,
    {
        {
            let pools = self.pools.lock().await;
            if let Some(pool) = pools.get(&key) {
                return Ok(pool.clone());
            }
        }

        let pool = Arc::new(create_fn().await?);

        {
            let mut pools = self.pools.lock().await;
            if let Some(existing) = pools.get(&key) {
                return Ok(existing.clone());
            }
            pools.insert(key, pool.clone());
        }

        Ok(pool)
    }

    /// Remove a pool by key (e.g. when the underlying connection is broken).
    pub async fn remove(&self, key: &PoolKey) {
        let mut pools = self.pools.lock().await;
        pools.remove(key);
    }

    /// Health check + automatic removal of dead pools.
    ///
    /// Uses a snapshot-based approach: takes a copy of all pools under lock,
    /// then runs `check_fn` without holding the lock (check may involve
    /// network I/O). Pools that fail the check are removed in a final
    /// lock step. Safe for concurrent access from multiple tabs.
    pub async fn health_check<F>(&self, mut check_fn: F)
    where
        F: FnMut(&T) -> bool,
    {
        let snapshot: Vec<(PoolKey, Arc<T>)> = {
            let pools = self.pools.lock().await;
            pools
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };

        let dead_keys: Vec<PoolKey> = snapshot
            .into_iter()
            .filter_map(|(key, pool)| {
                if !check_fn(&pool) {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();

        if !dead_keys.is_empty() {
            let mut pools = self.pools.lock().await;
            for key in dead_keys {
                pools.remove(&key);
            }
        }
    }

    /// Remove all pools (e.g. on application shutdown).
    pub async fn clear(&self) {
        let mut pools = self.pools.lock().await;
        pools.clear();
    }

    /// Clean up pools that are no longer referenced by any tab.
    ///
    /// A pool is considered unused when its `Arc::strong_count` equals 1
    /// (only the manager's own HashMap entry holds a reference).
    pub async fn cleanup(&self) {
        let mut pools = self.pools.lock().await;
        pools.retain(|_, pool| Arc::strong_count(pool) > 1);
    }

    /// Return the number of pools currently managed.
    pub async fn pool_count(&self) -> usize {
        let pools = self.pools.lock().await;
        pools.len()
    }

    /// Return the number of strong references to a specific pool (0 if not present).
    pub async fn ref_count(&self, key: &PoolKey) -> usize {
        let pools = self.pools.lock().await;
        pools.get(key).map(|p| Arc::strong_count(p)).unwrap_or(0)
    }
}

impl<T> Default for ConnectionPoolManager<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_or_create_returns_same_pool_for_same_key() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();
        let key = PoolKey::new("inst1", "conn1");

        let pool1 = manager
            .get_or_create(key.clone(), || async { Ok::<String, ()>("pool-data".to_string()) })
            .await
            .unwrap();
        let pool2 = manager
            .get_or_create(key, || async { Ok::<String, ()>("other".to_string()) })
            .await
            .unwrap();

        assert_eq!(pool1.as_str(), "pool-data");
        assert_eq!(pool2.as_str(), "pool-data");
        assert!(Arc::ptr_eq(&pool1, &pool2));
    }

    #[tokio::test]
    async fn different_keys_create_different_pools() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();

        let pool1 = manager
            .get_or_create(PoolKey::new("inst1", "conn1"), || async {
                Ok::<String, ()>("pool1".to_string())
            })
            .await
            .unwrap();
        let pool2 = manager
            .get_or_create(PoolKey::new("inst1", "conn2"), || async {
                Ok::<String, ()>("pool2".to_string())
            })
            .await
            .unwrap();

        assert_eq!(pool1.as_str(), "pool1");
        assert_eq!(pool2.as_str(), "pool2");
        assert!(!Arc::ptr_eq(&pool1, &pool2));
    }

    #[tokio::test]
    async fn cleanup_removes_unused_pools() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();

        let pool = manager
            .get_or_create(PoolKey::new("inst1", "conn1"), || async {
                Ok::<String, ()>("pool".to_string())
            })
            .await
            .unwrap();

        assert_eq!(manager.pool_count().await, 1);

        drop(pool);
        manager.cleanup().await;

        assert_eq!(manager.pool_count().await, 0);
    }

    #[tokio::test]
    async fn cleanup_keeps_shared_pools() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();

        let pool1 = manager
            .get_or_create(PoolKey::new("inst1", "conn1"), || async {
                Ok::<String, ()>("pool".to_string())
            })
            .await
            .unwrap();
        let _pool2 = manager
            .get_or_create(PoolKey::new("inst1", "conn1"), || async {
                Ok::<String, ()>("unused".to_string())
            })
            .await
            .unwrap();

        drop(pool1);
        manager.cleanup().await;

        assert_eq!(manager.pool_count().await, 1);
        assert!(manager.ref_count(&PoolKey::new("inst1", "conn1")).await > 1);
    }

    #[tokio::test]
    async fn remove_deletes_pool() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();
        let key = PoolKey::new("inst1", "conn1");

        let _pool = manager
            .get_or_create(key.clone(), || async { Ok::<String, ()>("data".to_string()) })
            .await
            .unwrap();

        assert_eq!(manager.pool_count().await, 1);
        manager.remove(&key).await;
        assert_eq!(manager.pool_count().await, 0);
    }

    #[tokio::test]
    async fn get_or_create_propagates_errors() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();
        let key = PoolKey::new("inst1", "conn1");

        let result = manager
            .get_or_create(key.clone(), || async { Err("create failed".to_string()) })
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "create failed");

        assert_eq!(manager.pool_count().await, 0);
    }

    #[tokio::test]
    async fn get_or_create_after_error_recreates() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();
        let key = PoolKey::new("inst1", "conn1");

        let _ = manager
            .get_or_create(key.clone(), || async { Err("fail".to_string()) })
            .await;

        let pool = manager
            .get_or_create(key, || async { Ok::<String, ()>("success".to_string()) })
            .await
            .unwrap();
        assert_eq!(pool.as_str(), "success");
    }

    #[tokio::test]
    async fn health_check_removes_dead_pools() {
        let manager: ConnectionPoolManager<String> = ConnectionPoolManager::new();

        let _alive = manager
            .get_or_create(PoolKey::new("inst1", "conn1"), || async {
                Ok::<String, ()>("alive".to_string())
            })
            .await
            .unwrap();
        let _dead = manager
            .get_or_create(PoolKey::new("inst1", "conn2"), || async {
                Ok::<String, ()>("dead".to_string())
            })
            .await
            .unwrap();

        assert_eq!(manager.pool_count().await, 2);

        manager.health_check(|pool| pool.as_str() == "alive").await;

        assert_eq!(manager.pool_count().await, 1);
    }
}
