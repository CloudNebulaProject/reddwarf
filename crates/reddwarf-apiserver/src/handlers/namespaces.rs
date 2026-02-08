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
use reddwarf_core::{GroupVersionKind, Namespace, ResourceKey};
use reddwarf_storage::KeyEncoder;
use std::sync::Arc;
use tracing::info;

/// GET /api/v1/namespaces/{name}
pub async fn get_namespace(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response> {
    let gvk = GroupVersionKind::from_api_version_kind("v1", "Namespace");
    let key = ResourceKey::cluster_scoped(gvk, name);

    let namespace: Namespace = get_resource(&state, &key).await?;

    Ok(ApiResponse::ok(namespace).into_response())
}

/// GET /api/v1/namespaces
pub async fn list_namespaces(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WatchParams>,
) -> Result<Response> {
    if params.is_watch() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Namespace");
        return Ok(watch_resource_stream(&state, gvk, None).into_response());
    }

    let prefix = KeyEncoder::encode_prefix("v1", "Namespace", None);
    let namespaces: Vec<Namespace> = list_resources(&state, &prefix).await?;

    let response = ListResponse::new("v1".to_string(), "NamespaceList".to_string(), namespaces);

    Ok(ApiResponse::ok(response).into_response())
}

/// POST /api/v1/namespaces
pub async fn create_namespace(
    State(state): State<Arc<AppState>>,
    Json(namespace): Json<Namespace>,
) -> Result<Response> {
    info!("Creating namespace");

    validate_resource(&namespace)?;

    let created = create_resource(&state, namespace).await?;

    Ok(ApiResponse::created(created).into_response())
}

/// PUT /api/v1/namespaces/{name}
pub async fn replace_namespace(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(mut namespace): Json<Namespace>,
) -> Result<Response> {
    info!("Replacing namespace: {}", name);

    namespace.metadata.name = Some(name);
    validate_resource(&namespace)?;

    let updated = update_resource(&state, namespace).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

/// DELETE /api/v1/namespaces/{name}
pub async fn delete_namespace(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response> {
    info!("Deleting namespace: {}", name);

    let gvk = GroupVersionKind::from_api_version_kind("v1", "Namespace");
    let key = ResourceKey::cluster_scoped(gvk, name.clone());

    delete_resource(&state, &key).await?;

    Ok(status_deleted(&name, "Namespace"))
}
