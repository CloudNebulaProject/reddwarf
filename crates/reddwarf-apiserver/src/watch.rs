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

/// Watch event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEvent<T> {
    #[serde(rename = "type")]
    pub event_type: WatchEventType,
    pub object: T,
}

impl<T> WatchEvent<T> {
    pub fn added(object: T) -> Self {
        Self {
            event_type: WatchEventType::Added,
            object,
        }
    }

    pub fn modified(object: T) -> Self {
        Self {
            event_type: WatchEventType::Modified,
            object,
        }
    }

    pub fn deleted(object: T) -> Self {
        Self {
            event_type: WatchEventType::Deleted,
            object,
        }
    }
}

// TODO: Implement full WATCH mechanism with SSE or WebSockets in future phase
// For MVP, we'll focus on GET/POST/PUT/PATCH/DELETE operations
