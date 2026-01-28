use crate::handlers::common::{create_resource, delete_resource, get_resource, list_resources, update_resource, ListResponse};
use crate::response::{status_deleted, ApiResponse};
use crate::validation::validate_resource;
use crate::{AppState, Result};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use reddwarf_core::{GroupVersionKind, ResourceKey, Service};
use reddwarf_storage::KeyEncoder;
use std::sync::Arc;
use tracing::info;

/// GET /api/v1/namespaces/{namespace}/services/{name}
pub async fn get_service(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Response> {
    let gvk = GroupVersionKind::from_api_version_kind("v1", "Service");
    let key = ResourceKey::new(gvk, namespace, name);

    let service: Service = get_resource(&state, &key).await?;

    Ok(ApiResponse::ok(service).into_response())
}

/// GET /api/v1/namespaces/{namespace}/services
pub async fn list_services(
    State(state): State<Arc<AppState>>,
    Path(namespace): Path<String>,
) -> Result<Response> {
    let prefix = KeyEncoder::encode_prefix("v1", "Service", Some(&namespace));
    let services: Vec<Service> = list_resources(&state, &prefix).await?;

    let response = ListResponse::new("v1".to_string(), "ServiceList".to_string(), services);

    Ok(ApiResponse::ok(response).into_response())
}

/// POST /api/v1/namespaces/{namespace}/services
pub async fn create_service(
    State(state): State<Arc<AppState>>,
    Path(namespace): Path<String>,
    Json(mut service): Json<Service>,
) -> Result<Response> {
    info!("Creating service in namespace: {}", namespace);

    service.metadata.namespace = Some(namespace);
    validate_resource(&service)?;

    let created = create_resource(&state, service).await?;

    Ok(ApiResponse::created(created).into_response())
}

/// PUT /api/v1/namespaces/{namespace}/services/{name}
pub async fn replace_service(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
    Json(mut service): Json<Service>,
) -> Result<Response> {
    info!("Replacing service: {}/{}", namespace, name);

    service.metadata.namespace = Some(namespace);
    service.metadata.name = Some(name);
    validate_resource(&service)?;

    let updated = update_resource(&state, service).await?;

    Ok(ApiResponse::ok(updated).into_response())
}

/// DELETE /api/v1/namespaces/{namespace}/services/{name}
pub async fn delete_service(
    State(state): State<Arc<AppState>>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Response> {
    info!("Deleting service: {}/{}", namespace, name);

    let gvk = GroupVersionKind::from_api_version_kind("v1", "Service");
    let key = ResourceKey::new(gvk, namespace, name.clone());

    delete_resource(&state, &key).await?;

    Ok(status_deleted(&name, "Service"))
}
