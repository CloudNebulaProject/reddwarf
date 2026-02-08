use crate::event_bus::{EventBusConfig, ResourceEvent};
use reddwarf_storage::RedbBackend;
use reddwarf_versioning::VersionStore;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Storage backend
    pub storage: Arc<RedbBackend>,

    /// Version store
    pub version_store: Arc<VersionStore>,

    /// Event bus sender — broadcast channel for resource mutation events
    pub event_tx: broadcast::Sender<ResourceEvent>,
}

impl AppState {
    /// Create a new AppState with default event bus config
    pub fn new(storage: Arc<RedbBackend>, version_store: Arc<VersionStore>) -> Self {
        Self::with_event_bus_config(storage, version_store, EventBusConfig::default())
    }

    /// Create a new AppState with custom event bus config
    pub fn with_event_bus_config(
        storage: Arc<RedbBackend>,
        version_store: Arc<VersionStore>,
        config: EventBusConfig,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(config.capacity);
        Self {
            storage,
            version_store,
            event_tx,
        }
    }

    /// Subscribe to resource events
    pub fn subscribe(&self) -> broadcast::Receiver<ResourceEvent> {
        self.event_tx.subscribe()
    }
}
