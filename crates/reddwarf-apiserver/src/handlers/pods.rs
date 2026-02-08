use crate::handlers::common::{
    create_resource, delete_resource, get_resource, list_resources, update_resource, ListResponse,
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
pub async fn delete_pod(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Response> {
    info!("Deleting pod: {}/{}", namespace, name);

    let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
    let key = ResourceKey::new(gvk, namespace, name.clone());

    delete_resource(&state, &key).await?;

    Ok(status_deleted(&name, "Pod"))
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

    #[tokio::test]
    async fn test_create_and_get_pod() {
        let state = setup_state().await;

        // Create pod
        let mut pod = Pod::default();
        pod.metadata.name = Some("test-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(Default::default());
        pod.spec.as_mut().unwrap().containers = vec![Default::default()];

        let created = create_resource(&*state, pod).await.unwrap();
        assert!(created.resource_version().is_some());

        // Get pod
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "test-pod");
        let retrieved: Pod = get_resource(&*state, &key).await.unwrap();

        assert_eq!(retrieved.metadata.name, Some("test-pod".to_string()));
    }

    #[tokio::test]
    async fn test_list_pods() {
        let state = setup_state().await;

        // Create multiple pods
        for i in 0..3 {
            let mut pod = Pod::default();
            pod.metadata.name = Some(format!("test-pod-{}", i));
            pod.metadata.namespace = Some("default".to_string());
            pod.spec = Some(Default::default());
            pod.spec.as_mut().unwrap().containers = vec![Default::default()];

            create_resource(&*state, pod).await.unwrap();
        }

        // List pods
        let prefix = KeyEncoder::encode_prefix("v1", "Pod", Some("default"));
        let pods: Vec<Pod> = list_resources(&*state, &prefix).await.unwrap();

        assert_eq!(pods.len(), 3);
    }
}
