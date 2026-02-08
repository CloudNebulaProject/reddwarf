use crate::types::{GroupVersionKind, ResourceKey};
use serde::{Deserialize, Serialize};

/// Watch event type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum WatchEventType {
    Added,
    Modified,
    Deleted,
    Error,
}

/// A resource event emitted by the API server on mutations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEvent {
    /// Type of watch event (ADDED, MODIFIED, DELETED)
    pub event_type: WatchEventType,
    /// GroupVersionKind of the resource
    pub gvk: GroupVersionKind,
    /// Full resource key (gvk + namespace + name)
    pub resource_key: ResourceKey,
    /// The serialized resource object
    pub object: serde_json::Value,
    /// Resource version at the time of the event
    pub resource_version: String,
}

impl ResourceEvent {
    /// Create an ADDED event
    pub fn added(
        resource_key: ResourceKey,
        object: serde_json::Value,
        resource_version: String,
    ) -> Self {
        Self {
            event_type: WatchEventType::Added,
            gvk: resource_key.gvk.clone(),
            resource_key,
            object,
            resource_version,
        }
    }

    /// Create a MODIFIED event
    pub fn modified(
        resource_key: ResourceKey,
        object: serde_json::Value,
        resource_version: String,
    ) -> Self {
        Self {
            event_type: WatchEventType::Modified,
            gvk: resource_key.gvk.clone(),
            resource_key,
            object,
            resource_version,
        }
    }

    /// Create a DELETED event
    pub fn deleted(
        resource_key: ResourceKey,
        object: serde_json::Value,
        resource_version: String,
    ) -> Self {
        Self {
            event_type: WatchEventType::Deleted,
            gvk: resource_key.gvk.clone(),
            resource_key,
            object,
            resource_version,
        }
    }
}
