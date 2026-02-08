use crate::event_bus::ResourceEvent;
use crate::AppState;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::StreamExt;
use reddwarf_core::GroupVersionKind;
pub use reddwarf_core::WatchEventType;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};

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

/// Query parameters for watch requests
#[derive(Debug, Deserialize, Default)]
pub struct WatchParams {
    /// Set to "true" or "1" to enable watch mode
    pub watch: Option<String>,
    /// Resource version to start watching from
    #[serde(rename = "resourceVersion")]
    pub resource_version: Option<String>,
}

impl WatchParams {
    /// Check if this is a watch request
    pub fn is_watch(&self) -> bool {
        self.watch
            .as_deref()
            .is_some_and(|v| v == "true" || v == "1")
    }
}

/// Kubernetes wire-format watch event for SSE
#[derive(Serialize)]
struct SseWatchEvent {
    #[serde(rename = "type")]
    event_type: WatchEventType,
    object: serde_json::Value,
}

impl From<&ResourceEvent> for SseWatchEvent {
    fn from(event: &ResourceEvent) -> Self {
        Self {
            event_type: event.event_type.clone(),
            object: event.object.clone(),
        }
    }
}

/// Create an SSE stream that watches for resource events filtered by GVK and optional namespace
pub fn watch_resource_stream(
    state: &Arc<AppState>,
    gvk: GroupVersionKind,
    namespace: Option<String>,
) -> Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>> {
    let rx = state.subscribe();
    let stream = BroadcastStream::new(rx);

    let filtered = stream.filter_map(
        move |result: std::result::Result<ResourceEvent, BroadcastStreamRecvError>| {
            let gvk = gvk.clone();
            let namespace = namespace.clone();
            async move {
                let event = result.ok()?;

                // Filter by GVK
                if event.gvk != gvk {
                    return None;
                }

                // Filter by namespace if specified
                if let Some(ref ns) = namespace {
                    if event.resource_key.namespace != *ns {
                        return None;
                    }
                }

                let sse_event = SseWatchEvent::from(&event);
                let data = serde_json::to_string(&sse_event).ok()?;
                Some(Ok(Event::default().data(data)))
            }
        },
    );

    Sse::new(filtered).keep_alive(KeepAlive::default())
}
