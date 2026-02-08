pub use reddwarf_core::{ResourceEvent, WatchEventType};

/// Configuration for the event bus
#[derive(Debug, Clone)]
pub struct EventBusConfig {
    /// Capacity of the broadcast channel
    pub capacity: usize,
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self { capacity: 4096 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::common::{create_resource, delete_resource, update_resource};
    use crate::AppState;
    use reddwarf_core::{GroupVersionKind, Pod, Resource, ResourceKey, WatchEventType};
    use reddwarf_storage::RedbBackend;
    use reddwarf_versioning::VersionStore;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn make_state() -> Arc<AppState> {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let storage = Arc::new(RedbBackend::new(&db_path).unwrap());
        let version_store = Arc::new(VersionStore::new(storage.clone()).unwrap());
        Arc::new(AppState::new(storage, version_store))
    }

    fn make_test_pod(name: &str, namespace: &str) -> Pod {
        let mut pod = Pod::default();
        pod.metadata.name = Some(name.to_string());
        pod.metadata.namespace = Some(namespace.to_string());
        pod.spec = Some(Default::default());
        pod.spec.as_mut().unwrap().containers = vec![Default::default()];
        pod
    }

    #[test]
    fn test_resource_event_serde_roundtrip() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "nginx");
        let object =
            serde_json::json!({"apiVersion": "v1", "kind": "Pod", "metadata": {"name": "nginx"}});

        let event = ResourceEvent::added(key, object.clone(), "abc123".to_string());

        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: ResourceEvent = serde_json::from_str(&serialized).unwrap();

        assert!(matches!(deserialized.event_type, WatchEventType::Added));
        assert_eq!(deserialized.resource_key.name, "nginx");
        assert_eq!(deserialized.resource_key.namespace, "default");
        assert_eq!(deserialized.gvk.kind, "Pod");
        assert_eq!(deserialized.object, object);
        assert_eq!(deserialized.resource_version, "abc123");
    }

    #[test]
    fn test_event_bus_config_default() {
        let config = EventBusConfig::default();
        assert_eq!(config.capacity, 4096);
    }

    #[tokio::test]
    async fn test_subscribe_receives_events() {
        let state = make_state();
        let mut rx = state.subscribe();

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "nginx");
        let object = serde_json::json!({"kind": "Pod"});
        let event = ResourceEvent::added(key, object, "v1".to_string());

        state.event_tx.send(event).unwrap();

        let received = rx.recv().await.unwrap();
        assert!(matches!(received.event_type, WatchEventType::Added));
        assert_eq!(received.resource_key.name, "nginx");
    }

    #[tokio::test]
    async fn test_multiple_subscribers_each_get_copy() {
        let state = make_state();
        let mut rx1 = state.subscribe();
        let mut rx2 = state.subscribe();

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "test");
        let event = ResourceEvent::added(key, serde_json::json!({}), "v1".to_string());

        state.event_tx.send(event).unwrap();

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert_eq!(e1.resource_key.name, "test");
        assert_eq!(e2.resource_key.name, "test");
    }

    #[tokio::test]
    async fn test_event_published_after_create() {
        let state = make_state();
        let mut rx = state.subscribe();

        let pod = make_test_pod("create-test", "default");
        create_resource(&*state, pod).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(event.event_type, WatchEventType::Added));
        assert_eq!(event.resource_key.name, "create-test");
        assert_eq!(event.gvk.kind, "Pod");
    }

    #[tokio::test]
    async fn test_event_published_after_update() {
        let state = make_state();

        let pod = make_test_pod("update-test", "default");
        let created = create_resource(&*state, pod).await.unwrap();

        // Subscribe after create so we only get the update event
        let mut rx = state.subscribe();

        let updated = update_resource(&*state, created).await.unwrap();
        assert!(updated.resource_version().is_some());

        let event = rx.recv().await.unwrap();
        assert!(matches!(event.event_type, WatchEventType::Modified));
        assert_eq!(event.resource_key.name, "update-test");
    }

    #[tokio::test]
    async fn test_event_published_after_delete() {
        let state = make_state();

        let pod = make_test_pod("delete-test", "default");
        create_resource(&*state, pod).await.unwrap();

        // Subscribe after create
        let mut rx = state.subscribe();

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "delete-test");
        delete_resource(&*state, &key).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(event.event_type, WatchEventType::Deleted));
        assert_eq!(event.resource_key.name, "delete-test");
    }

    #[tokio::test]
    async fn test_watch_namespace_filter() {
        let state = make_state();
        let mut rx = state.subscribe();

        // Create pods in two different namespaces
        let pod1 = make_test_pod("pod-ns1", "namespace-a");
        let pod2 = make_test_pod("pod-ns2", "namespace-b");
        create_resource(&*state, pod1).await.unwrap();
        create_resource(&*state, pod2).await.unwrap();

        // Receive both events
        let event1 = rx.recv().await.unwrap();
        let event2 = rx.recv().await.unwrap();

        // Verify we can filter by namespace
        let events = vec![event1, event2];
        let ns_a_events: Vec<_> = events
            .iter()
            .filter(|e| e.resource_key.namespace == "namespace-a")
            .collect();
        let ns_b_events: Vec<_> = events
            .iter()
            .filter(|e| e.resource_key.namespace == "namespace-b")
            .collect();

        assert_eq!(ns_a_events.len(), 1);
        assert_eq!(ns_b_events.len(), 1);
        assert_eq!(ns_a_events[0].resource_key.name, "pod-ns1");
        assert_eq!(ns_b_events[0].resource_key.name, "pod-ns2");
    }
}
