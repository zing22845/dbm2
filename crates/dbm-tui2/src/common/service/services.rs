//! The service bundle injected into effects.

use std::sync::{Arc, Mutex};

use dbm_store::Store;

/// Aggregated infrastructure dependencies available to effects.
///
/// Constructed once at the composition root and shared with the effect runner
/// via `Arc`. `Store` holds a `rusqlite::Connection` which is `Send` but not
/// `Sync`, so it is wrapped in `Arc<Mutex<Store>>` to be safely shared across
/// the concurrent effect tasks.
#[derive(Clone)]
pub struct Services {
    /// The discovery/session store (SQLite-backed).
    pub store: Arc<Mutex<Store>>,
}

impl Services {
    /// Create the service bundle with a default local store.
    ///
    /// Called once at the composition root (`app::run_async`).
    pub fn new() -> anyhow::Result<Self> {
        let store = Store::open_default()?;
        Ok(Services {
            store: Arc::new(Mutex::new(store)),
        })
    }
}

impl Default for Services {
    fn default() -> Self {
        // In-memory store for tests / when no disk store is wanted. Falls back
        // to a fresh in-memory store if the default location is unavailable.
        let store = Store::open_in_memory()
            .or_else(|_| Store::open_default())
            .expect("failed to open an in-memory store");
        Services {
            store: Arc::new(Mutex::new(store)),
        }
    }
}
