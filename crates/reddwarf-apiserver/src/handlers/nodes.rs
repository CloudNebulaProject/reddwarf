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
use reddwarf_core::{GroupVersionKind, Node, ResourceKey};
use reddwarf_storage::KeyEncoder;
use std::sync::Arc;
use tracing::info;

/// GET /api/v1/nodes/{name}
pub async fn get_node(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response> {
    let gvk = GroupVersionKind::from_api_version_kind("v1", "Node");
    let key = ResourceKey::cluster_scoped(gvk, name);

    let node: Node = get_resource(&state, &key).await?;

    Ok(ApiResponse::ok(node).into_response())
}

/// GET /api/v1/nodes
pub async fn list_nodes(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WatchParams>,
) -> Result<Response> {
    if params.is_watch() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Node");
        return Ok(watch_resource_stream(&state, gvk, None).into_response());
    }

    let prefix = KeyEncoder::encode_prefix("v1", "Node", None);
    let nodes: Vec<Node> = list_resources(&state, &prefix).await?;

    let response = ListResponse::new("v1".to_string(), "NodeList".to_string(), nodes);

    Ok(ApiResponse::ok(response).into_response())
}

/// POST /api/v1/nodes
pub async fn create_node(
    State(state): State<Arc<AppState>>,
    Json(node): Json<Node>,
) -> Result<Response> {
    info!("Creating node");

    validate_resource(&node)?;

    let created = create_resource(&state, node).await?;

    Ok(ApiResponse::created(created).into_response())
}

/// PUT /api/v1/nodes/{name}
pub async fn replace_node(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(mut node): Json<Node>,
) -> Result<Response> {
    info!("Replacing node: {}", name);

    node.metadata.name = Some(name);
    validate_resource(&node)?;

    let updated = update_resource(&state, node).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

/// PUT /api/v1/nodes/{name}/status
pub async fn update_node_status(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(mut node): Json<Node>,
) -> Result<Response> {
    info!("Updating node status: {}", name);

    node.metadata.name = Some(name);

    let updated = update_status(&state, node).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

/// DELETE /api/v1/nodes/{name}
pub async fn delete_node(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response> {
    info!("Deleting node: {}", name);

    let gvk = GroupVersionKind::from_api_version_kind("v1", "Node");
    let key = ResourceKey::cluster_scoped(gvk, name.clone());

    delete_resource(&state, &key).await?;

    Ok(status_deleted(&name, "Node"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_core::k8s_openapi::api::core::v1::{NodeCondition, NodeStatus};
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
    async fn test_update_node_status_updates_conditions() {
        let state = setup_state().await;

        // Create a node
        let mut node = Node::default();
        node.metadata.name = Some("test-node".to_string());
        let created = create_resource(&*state, node).await.unwrap();

        // Update status with conditions
        let mut status_node = created.clone();
        status_node.status = Some(NodeStatus {
            conditions: Some(vec![NodeCondition {
                type_: "Ready".to_string(),
                status: "True".to_string(),
                reason: Some("KubeletReady".to_string()),
                message: Some("node is healthy".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        });

        let updated = update_status(&*state, status_node).await.unwrap();

        let conditions = updated
            .status
            .as_ref()
            .unwrap()
            .conditions
            .as_ref()
            .unwrap();
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].type_, "Ready");
        assert_eq!(conditions[0].status, "True");

        // Resource version should be bumped
        assert_ne!(updated.resource_version(), created.resource_version());
    }
}
