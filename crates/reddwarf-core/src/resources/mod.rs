pub mod quantities;

pub use quantities::ResourceQuantities;

use crate::{GroupVersionKind, ResourceKey, ResourceVersion};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::{Deserialize, Serialize};

/// Base validation for all resources
pub fn validate_base(metadata: &ObjectMeta) -> Result<(), ResourceError> {
    if metadata.name.is_none() {
        return Err(ResourceError::MissingField("metadata.name".to_string()));
    }

    if let Some(name) = &metadata.name {
        if !is_valid_name(name) {
            return Err(ResourceError::InvalidName(name.clone()));
        }
    }

    Ok(())
}

/// Trait for Kubernetes resources
pub trait Resource: Serialize + for<'de> Deserialize<'de> + Send + Sync {
    /// Get the API version of this resource
    fn api_version(&self) -> String;

    /// Get the kind of this resource
    fn kind(&self) -> String;

    /// Get the metadata of this resource
    fn metadata(&self) -> &ObjectMeta;

    /// Get mutable metadata
    fn metadata_mut(&mut self) -> &mut ObjectMeta;

    /// Get the GroupVersionKind
    fn gvk(&self) -> GroupVersionKind {
        GroupVersionKind::from_api_version_kind(&self.api_version(), &self.kind())
    }

    /// Get the ResourceKey
    fn resource_key(&self) -> Result<ResourceKey, ResourceError> {
        let metadata = self.metadata();
        let name = metadata
            .name
            .as_ref()
            .ok_or_else(|| ResourceError::MissingField("metadata.name".to_string()))?;
        let namespace = metadata.namespace.clone().unwrap_or_default();

        Ok(ResourceKey::new(self.gvk(), namespace, name))
    }

    /// Get the resource version
    fn resource_version(&self) -> Option<ResourceVersion> {
        self.metadata()
            .resource_version
            .as_ref()
            .map(ResourceVersion::new)
    }

    /// Set the resource version
    fn set_resource_version(&mut self, version: ResourceVersion) {
        self.metadata_mut().resource_version = Some(version.0);
    }

    /// Get the UID
    fn uid(&self) -> Option<String> {
        self.metadata().uid.clone()
    }

    /// Set the UID
    fn set_uid(&mut self, uid: String) {
        self.metadata_mut().uid = Some(uid);
    }

    /// Check if this is a namespaced resource
    fn is_namespaced(&self) -> bool {
        self.metadata().namespace.is_some()
    }

    /// Validate the resource
    fn validate(&self) -> Result<(), ResourceError> {
        validate_base(self.metadata())
    }
}

/// Resource-related errors
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid resource name: {0}")]
    InvalidName(String),

    #[error("Invalid namespace: {0}")]
    InvalidNamespace(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

/// Validate a Kubernetes resource name (DNS-1123 subdomain)
pub fn is_valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 253 {
        return false;
    }

    // Must contain only lowercase alphanumeric, '-', or '.'
    // Must start and end with alphanumeric
    let chars: Vec<char> = name.chars().collect();

    if !chars[0].is_ascii_lowercase() && !chars[0].is_ascii_digit() {
        return false;
    }

    if !chars[chars.len() - 1].is_ascii_lowercase() && !chars[chars.len() - 1].is_ascii_digit() {
        return false;
    }

    chars
        .iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '.')
}

// Implement Resource trait for common k8s-openapi types
use k8s_openapi::api::core::v1::{Namespace, Node, Pod, Service};

impl Resource for Pod {
    fn api_version(&self) -> String {
        "v1".to_string()
    }

    fn kind(&self) -> String {
        "Pod".to_string()
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }

    fn validate(&self) -> Result<(), ResourceError> {
        // Call base validation
        validate_base(&self.metadata)?;

        // Pod-specific validation
        if let Some(spec) = &self.spec {
            if spec.containers.is_empty() {
                return Err(ResourceError::ValidationFailed(
                    "Pod must have at least one container".to_string(),
                ));
            }
        } else {
            return Err(ResourceError::MissingField("spec".to_string()));
        }

        Ok(())
    }
}

impl Resource for Node {
    fn api_version(&self) -> String {
        "v1".to_string()
    }

    fn kind(&self) -> String {
        "Node".to_string()
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

impl Resource for Service {
    fn api_version(&self) -> String {
        "v1".to_string()
    }

    fn kind(&self) -> String {
        "Service".to_string()
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

impl Resource for Namespace {
    fn api_version(&self) -> String {
        "v1".to_string()
    }

    fn kind(&self) -> String {
        "Namespace".to_string()
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }

    fn is_namespaced(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_name() {
        assert!(is_valid_name("nginx"));
        assert!(is_valid_name("my-app"));
        assert!(is_valid_name("my-app-123"));
        assert!(is_valid_name("my.app"));

        assert!(!is_valid_name(""));
        assert!(!is_valid_name("MyApp")); // uppercase
        assert!(!is_valid_name("-myapp")); // starts with dash
        assert!(!is_valid_name("myapp-")); // ends with dash
        assert!(!is_valid_name("my_app")); // underscore
    }

    #[test]
    fn test_pod_resource_key() {
        let mut pod = Pod::default();
        pod.metadata.name = Some("nginx".to_string());
        pod.metadata.namespace = Some("default".to_string());

        let key = pod.resource_key().unwrap();
        assert_eq!(key.name, "nginx");
        assert_eq!(key.namespace, "default");
        assert_eq!(key.gvk.kind, "Pod");
    }
}
