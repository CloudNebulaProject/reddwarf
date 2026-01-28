use reddwarf_storage::RedbBackend;
use reddwarf_versioning::VersionStore;
use std::sync::Arc;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Storage backend
    pub storage: Arc<RedbBackend>,

    /// Version store
    pub version_store: Arc<VersionStore>,
}

impl AppState {
    /// Create a new AppState
    pub fn new(storage: Arc<RedbBackend>, version_store: Arc<VersionStore>) -> Self {
        Self {
            storage,
            version_store,
        }
    }
}
