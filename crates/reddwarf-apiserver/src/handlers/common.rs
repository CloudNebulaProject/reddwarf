use crate::{ApiError, AppState, Result};
use reddwarf_core::{Resource, ResourceKey};
use reddwarf_storage::{KVStore, KeyEncoder};
use reddwarf_versioning::{Change, CommitBuilder};
use serde::Serialize;
use tracing::{debug, info};
use uuid::Uuid;

/// Get a resource from storage
pub async fn get_resource<T: Resource>(state: &AppState, key: &ResourceKey) -> Result<T> {
    debug!("Getting resource: {}", key);

    let storage_key = KeyEncoder::encode_resource_key(key);
    let data = state
        .storage
        .as_ref()
        .get(storage_key.as_bytes())?
        .ok_or_else(|| ApiError::NotFound(format!("Resource not found: {}", key)))?;

    let resource: T = serde_json::from_slice(&data)?;
    Ok(resource)
}

/// Create a resource in storage
pub async fn create_resource<T: Resource>(state: &AppState, mut resource: T) -> Result<T> {
    let key = resource
        .resource_key()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    info!("Creating resource: {}", key);

    // Check if resource already exists
    let storage_key = KeyEncoder::encode_resource_key(&key);
    if state.storage.as_ref().exists(storage_key.as_bytes())? {
        return Err(ApiError::AlreadyExists(format!(
            "Resource already exists: {}",
            key
        )));
    }

    // Set UID and initial resource version
    resource.set_uid(Uuid::new_v4().to_string());

    // Serialize resource
    let data = serde_json::to_vec(&resource)?;

    // Create commit
    let change = Change::create(
        storage_key.clone(),
        String::from_utf8_lossy(&data).to_string(),
    );

    let commit = state
        .version_store
        .create_commit(
            CommitBuilder::new()
                .change(change)
                .message(format!("Create {}", key)),
        )
        .map_err(ApiError::from)?;

    // Set resource version to commit ID
    resource.set_resource_version(reddwarf_core::ResourceVersion::new(commit.id().to_string()));

    // Store in storage
    state.storage.as_ref().put(storage_key.as_bytes(), &data)?;

    info!("Created resource: {} with version {}", key, commit.id());
    Ok(resource)
}

/// Update a resource in storage
pub async fn update_resource<T: Resource>(state: &AppState, mut resource: T) -> Result<T> {
    let key = resource
        .resource_key()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    info!("Updating resource: {}", key);

    let storage_key = KeyEncoder::encode_resource_key(&key);

    // Get previous version
    let prev_data = state
        .storage
        .as_ref()
        .get(storage_key.as_bytes())?
        .ok_or_else(|| ApiError::NotFound(format!("Resource not found: {}", key)))?;

    // Serialize new resource
    let new_data = serde_json::to_vec(&resource)?;

    // Create commit
    let change = Change::update(
        storage_key.clone(),
        String::from_utf8_lossy(&new_data).to_string(),
        String::from_utf8_lossy(&prev_data).to_string(),
    );

    let commit = state
        .version_store
        .create_commit(
            CommitBuilder::new()
                .change(change)
                .message(format!("Update {}", key)),
        )
        .map_err(ApiError::from)?;

    // Set resource version to commit ID
    resource.set_resource_version(reddwarf_core::ResourceVersion::new(commit.id().to_string()));

    // Update in storage
    state
        .storage
        .as_ref()
        .put(storage_key.as_bytes(), &new_data)?;

    info!("Updated resource: {} with version {}", key, commit.id());
    Ok(resource)
}

/// Delete a resource from storage
pub async fn delete_resource(state: &AppState, key: &ResourceKey) -> Result<()> {
    info!("Deleting resource: {}", key);

    let storage_key = KeyEncoder::encode_resource_key(key);

    // Get current version
    let prev_data = state
        .storage
        .as_ref()
        .get(storage_key.as_bytes())?
        .ok_or_else(|| ApiError::NotFound(format!("Resource not found: {}", key)))?;

    // Create commit
    let change = Change::delete(
        storage_key.clone(),
        String::from_utf8_lossy(&prev_data).to_string(),
    );

    let commit = state
        .version_store
        .create_commit(
            CommitBuilder::new()
                .change(change)
                .message(format!("Delete {}", key)),
        )
        .map_err(ApiError::from)?;

    // Delete from storage
    state.storage.as_ref().delete(storage_key.as_bytes())?;

    info!("Deleted resource: {} at version {}", key, commit.id());
    Ok(())
}

/// List resources with optional filtering
pub async fn list_resources<T: Resource>(state: &AppState, prefix: &str) -> Result<Vec<T>> {
    debug!("Listing resources with prefix: {}", prefix);

    let results = state.storage.as_ref().scan(prefix.as_bytes())?;

    let mut resources = Vec::new();
    for (_key, data) in results.iter() {
        let resource: T = serde_json::from_slice(data)?;
        resources.push(resource);
    }

    debug!("Found {} resources", resources.len());
    Ok(resources)
}

/// List response wrapper
#[derive(Serialize)]
pub struct ListResponse<T: Serialize> {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub items: Vec<T>,
    pub metadata: ListMetadata,
}

/// List metadata
#[derive(Serialize)]
pub struct ListMetadata {
    #[serde(rename = "resourceVersion")]
    pub resource_version: String,
}

impl<T: Serialize> ListResponse<T> {
    pub fn new(api_version: String, kind: String, items: Vec<T>) -> Self {
        Self {
            api_version,
            kind,
            items,
            metadata: ListMetadata {
                resource_version: Uuid::new_v4().to_string(),
            },
        }
    }
}
