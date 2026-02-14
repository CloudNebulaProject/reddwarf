use crate::handlers::common::{
    create_resource, delete_resource, get_resource, list_resources, update_resource, update_status,
    ListResponse,
};
use crate::response::{status_deleted, ApiResponse};
use crate::validation::validate_resource;
use crate::watch::{watch_resource_stream, WatchParams};
use crate::{AppState, Result};
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use reddwarf_core::{GroupVersionKind, Pod, ResourceKey};
use reddwarf_storage::KeyEncoder;
use std::sync::Arc;
use tracing::info;

const DEFAULT_TERMINATION_GRACE_PERIOD: i64 = 30;

/// GET /api/v1/namespaces/{namespace}/pods/{name}
pub async fn get_pod(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Response> {
    let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
    let key = ResourceKey::new(gvk, namespace, name);

    let pod: Pod = get_resource(&state, &key).await?;

    Ok(ApiResponse::ok(pod).into_response())
}

/// GET /api/v1/namespaces/{namespace}/pods
/// GET /api/v1/pods (all namespaces)
pub async fn list_pods(
    State(state): State<Arc<AppState>>,
    Path(namespace): Path<Option<String>>,
    Query(params): Query<WatchParams>,
) -> Result<Response> {
    if params.is_watch() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        return Ok(watch_resource_stream(&state, gvk, namespace).into_response());
    }

    let prefix = if let Some(ns) = namespace {
        KeyEncoder::encode_prefix("v1", "Pod", Some(&ns))
    } else {
        KeyEncoder::encode_prefix("v1", "Pod", None)
    };

    let pods: Vec<Pod> = list_resources(&state, &prefix).await?;

    let response = ListResponse::new("v1".to_string(), "PodList".to_string(), pods);

    Ok(ApiResponse::ok(response).into_response())
}

/// POST /api/v1/namespaces/{namespace}/pods
pub async fn create_pod(
    State(state): State<Arc<AppState>>,
    Path(namespace): Path<String>,
    Json(mut pod): Json<Pod>,
) -> Result<Response> {
    info!("Creating pod in namespace: {}", namespace);

    // Ensure namespace matches
    pod.metadata.namespace = Some(namespace.clone());

    // Validate
    validate_resource(&pod)?;

    // Create
    let created = create_resource(&state, pod).await?;

    Ok(ApiResponse::created(created).into_response())
}

/// PUT /api/v1/namespaces/{namespace}/pods/{name}
pub async fn replace_pod(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
    Json(mut pod): Json<Pod>,
) -> Result<Response> {
    info!("Replacing pod: {}/{}", namespace, name);

    // Ensure metadata matches
    pod.metadata.namespace = Some(namespace.clone());
    pod.metadata.name = Some(name.clone());

    // Validate
    validate_resource(&pod)?;

    // Update
    let updated = update_resource(&state, pod).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

/// DELETE /api/v1/namespaces/{namespace}/pods/{name}
///
/// Initiates graceful termination: sets deletion_timestamp and phase=Terminating
/// instead of immediately removing the pod from storage. The controller will
/// drive the zone shutdown state machine and call finalize_pod() when cleanup
/// is complete.
pub async fn delete_pod(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Response> {
    info!("Deleting pod: {}/{}", namespace, name);

    let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
    let key = ResourceKey::new(gvk, namespace.clone(), name.clone());

    let mut pod: Pod = get_resource(&state, &key).await?;

    // Idempotent: if deletion_timestamp is already set, return current state
    if pod.metadata.deletion_timestamp.is_some() {
        info!(
            "Pod {}/{} already has deletion_timestamp set, returning current state",
            namespace, name
        );
        return Ok(ApiResponse::ok(pod).into_response());
    }

    // Set deletion metadata
    pod.metadata.deletion_timestamp = Some(
        reddwarf_core::k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(chrono::Utc::now()),
    );

    // Grace period from spec, defaulting to 30s
    let grace_period = pod
        .spec
        .as_ref()
        .and_then(|s| s.termination_grace_period_seconds)
        .unwrap_or(DEFAULT_TERMINATION_GRACE_PERIOD);
    pod.metadata.deletion_grace_period_seconds = Some(grace_period);

    // Set phase to Terminating
    let status = pod.status.get_or_insert_with(Default::default);
    status.phase = Some("Terminating".to_string());

    // Update resource — emits a MODIFIED event (not DELETED)
    let updated = update_resource(&state, pod).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

/// POST /api/v1/namespaces/{namespace}/pods/{name}/finalize
///
/// Performs the actual storage removal of a pod. Called by the controller after
/// zone cleanup is complete.
pub async fn finalize_pod(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Response> {
    info!("Finalizing pod: {}/{}", namespace, name);

    let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
    let key = ResourceKey::new(gvk, namespace, name.clone());

    delete_resource(&state, &key).await?;

    Ok(status_deleted(&name, "Pod"))
}

/// PUT /api/v1/namespaces/{namespace}/pods/{name}/status
pub async fn update_pod_status(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
    Json(mut pod): Json<Pod>,
) -> Result<Response> {
    info!("Updating pod status: {}/{}", namespace, name);

    // Ensure metadata matches the URL path
    pod.metadata.namespace = Some(namespace);
    pod.metadata.name = Some(name);

    let updated = update_status(&state, pod).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

/// PATCH /api/v1/namespaces/{namespace}/pods/{name}
pub async fn patch_pod(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
    Json(patch): Json<serde_json::Value>,
) -> Result<Response> {
    info!("Patching pod: {}/{}", namespace, name);

    // Get current pod
    let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
    let key = ResourceKey::new(gvk, namespace.clone(), name.clone());

    let mut pod: Pod = get_resource(&state, &key).await?;

    // Apply patch (simplified - just merge JSON)
    let mut pod_json = serde_json::to_value(&pod)?;
    json_patch::merge(&mut pod_json, &patch);

    // Deserialize back
    pod = serde_json::from_value(pod_json)?;

    // Validate
    validate_resource(&pod)?;

    // Update
    let updated = update_resource(&state, pod).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::WatchEventType;
    use reddwarf_core::k8s_openapi::api::core::v1::PodStatus;
    use reddwarf_core::Resource;
    use reddwarf_storage::RedbBackend;
    use reddwarf_versioning::VersionStore;
    use std::sync::Arc;
    use tempfile::tempdir;

    async fn setup_state() -> Arc<AppState> {
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

    #[tokio::test]
    async fn test_create_and_get_pod() {
        let state = setup_state().await;

        let pod = make_test_pod("test-pod", "default");
        let created = create_resource(&*state, pod).await.unwrap();
        assert!(created.resource_version().is_some());

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "test-pod");
        let retrieved: Pod = get_resource(&*state, &key).await.unwrap();

        assert_eq!(retrieved.metadata.name, Some("test-pod".to_string()));
    }

    #[tokio::test]
    async fn test_list_pods() {
        let state = setup_state().await;

        for i in 0..3 {
            let pod = make_test_pod(&format!("test-pod-{}", i), "default");
            create_resource(&*state, pod).await.unwrap();
        }

        let prefix = KeyEncoder::encode_prefix("v1", "Pod", Some("default"));
        let pods: Vec<Pod> = list_resources(&*state, &prefix).await.unwrap();

        assert_eq!(pods.len(), 3);
    }

    #[tokio::test]
    async fn test_update_pod_status_changes_phase_not_spec() {
        let state = setup_state().await;

        // Create a pod with spec
        let mut pod = make_test_pod("status-test", "default");
        pod.spec.as_mut().unwrap().containers[0].name = "nginx".to_string();
        let created = create_resource(&*state, pod).await.unwrap();
        let original_version = created.resource_version();

        // Update status only
        let mut status_pod = created.clone();
        status_pod.status = Some(PodStatus {
            phase: Some("Running".to_string()),
            pod_ip: Some("10.0.0.5".to_string()),
            ..Default::default()
        });

        let updated = update_status(&*state, status_pod).await.unwrap();

        // Status should be updated
        assert_eq!(
            updated.status.as_ref().unwrap().phase.as_deref(),
            Some("Running")
        );
        assert_eq!(
            updated.status.as_ref().unwrap().pod_ip.as_deref(),
            Some("10.0.0.5")
        );

        // Spec should be preserved
        assert_eq!(updated.spec.as_ref().unwrap().containers[0].name, "nginx");

        // Resource version should be bumped
        assert_ne!(updated.resource_version(), original_version);
    }

    #[tokio::test]
    async fn test_update_pod_status_bumps_resource_version() {
        let state = setup_state().await;

        let pod = make_test_pod("version-test", "default");
        let created = create_resource(&*state, pod).await.unwrap();
        let v1 = created.resource_version();

        // First status update
        let mut update1 = created.clone();
        update1.status = Some(PodStatus {
            phase: Some("Running".to_string()),
            ..Default::default()
        });
        let updated1 = update_status(&*state, update1).await.unwrap();
        let v2 = updated1.resource_version();

        assert_ne!(v1, v2);

        // Second status update
        let mut update2 = updated1.clone();
        update2.status = Some(PodStatus {
            phase: Some("Succeeded".to_string()),
            ..Default::default()
        });
        let updated2 = update_status(&*state, update2).await.unwrap();
        let v3 = updated2.resource_version();

        assert_ne!(v2, v3);
    }

    #[tokio::test]
    async fn test_update_pod_status_fires_modified_event() {
        let state = setup_state().await;

        let pod = make_test_pod("event-test", "default");
        let created = create_resource(&*state, pod).await.unwrap();

        // Subscribe after create
        let mut rx = state.subscribe();

        let mut status_pod = created;
        status_pod.status = Some(PodStatus {
            phase: Some("Running".to_string()),
            ..Default::default()
        });
        update_status(&*state, status_pod).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(event.event_type, WatchEventType::Modified));
        assert_eq!(event.resource_key.name, "event-test");
    }

    #[tokio::test]
    async fn test_delete_pod_sets_deletion_timestamp() {
        let state = setup_state().await;

        let pod = make_test_pod("graceful-pod", "default");
        create_resource(&*state, pod).await.unwrap();

        // Simulate what delete_pod handler does: read, set deletion_timestamp, update
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk.clone(), "default", "graceful-pod");

        let mut pod: Pod = get_resource(&*state, &key).await.unwrap();
        assert!(pod.metadata.deletion_timestamp.is_none());

        // Set deletion metadata
        pod.metadata.deletion_timestamp = Some(
            reddwarf_core::k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc::now(),
            ),
        );
        pod.metadata.deletion_grace_period_seconds = Some(30);
        let status = pod.status.get_or_insert_with(Default::default);
        status.phase = Some("Terminating".to_string());

        let updated = update_resource(&*state, pod).await.unwrap();

        // Pod should still exist in storage
        let retrieved: Pod = get_resource(&*state, &key).await.unwrap();
        assert!(retrieved.metadata.deletion_timestamp.is_some());
        assert_eq!(retrieved.metadata.deletion_grace_period_seconds, Some(30));
        assert_eq!(
            retrieved.status.as_ref().unwrap().phase.as_deref(),
            Some("Terminating")
        );

        // Should have emitted a MODIFIED event (not DELETED)
        assert!(updated.resource_version().is_some());
    }

    #[tokio::test]
    async fn test_delete_pod_idempotent() {
        let state = setup_state().await;

        let pod = make_test_pod("idem-pod", "default");
        create_resource(&*state, pod).await.unwrap();

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "idem-pod");

        // First delete: set deletion_timestamp
        let mut pod: Pod = get_resource(&*state, &key).await.unwrap();
        pod.metadata.deletion_timestamp = Some(
            reddwarf_core::k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc::now(),
            ),
        );
        pod.metadata.deletion_grace_period_seconds = Some(30);
        pod.status
            .get_or_insert_with(Default::default)
            .phase = Some("Terminating".to_string());
        update_resource(&*state, pod).await.unwrap();

        // Second "delete" attempt should see deletion_timestamp is already set
        let pod2: Pod = get_resource(&*state, &key).await.unwrap();
        assert!(pod2.metadata.deletion_timestamp.is_some());

        // Pod should still exist in storage (not removed)
        assert_eq!(
            pod2.status.as_ref().unwrap().phase.as_deref(),
            Some("Terminating")
        );
    }

    #[tokio::test]
    async fn test_finalize_removes_pod_from_storage() {
        let state = setup_state().await;

        let pod = make_test_pod("finalize-pod", "default");
        create_resource(&*state, pod).await.unwrap();

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "finalize-pod");

        // Pod should exist
        let _: Pod = get_resource(&*state, &key).await.unwrap();

        // Finalize (actual storage removal)
        delete_resource(&state, &key).await.unwrap();

        // Pod should be gone
        let result: std::result::Result<Pod, _> = get_resource(&*state, &key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_fires_modified_not_deleted_event() {
        let state = setup_state().await;

        let pod = make_test_pod("event-del-pod", "default");
        create_resource(&*state, pod).await.unwrap();

        // Subscribe to events
        let mut rx = state.subscribe();

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "event-del-pod");

        let mut pod: Pod = get_resource(&*state, &key).await.unwrap();
        pod.metadata.deletion_timestamp = Some(
            reddwarf_core::k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc::now(),
            ),
        );
        pod.status
            .get_or_insert_with(Default::default)
            .phase = Some("Terminating".to_string());
        update_resource(&*state, pod).await.unwrap();

        // Should get a MODIFIED event
        let event = rx.recv().await.unwrap();
        assert!(matches!(event.event_type, WatchEventType::Modified));
        assert_eq!(event.resource_key.name, "event-del-pod");
    }
}
