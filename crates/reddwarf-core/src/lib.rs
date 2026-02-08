//! Reddwarf Core - Fundamental types and traits for the Reddwarf Kubernetes control plane
//!
//! This crate provides:
//! - Core Kubernetes resource abstractions
//! - Error types with miette diagnostics
//! - Type-safe resource keys and identifiers
//! - Serialization helpers

pub mod error;
pub mod events;
pub mod resources;
pub mod types;

// Re-export commonly used types
pub use error::{ReddwarfError, Result};
pub use events::{ResourceEvent, WatchEventType};
pub use resources::{is_valid_name, Resource, ResourceError};
pub use types::{GroupVersionKind, ResourceKey, ResourceVersion};

// Re-export k8s-openapi types for convenience
pub use k8s_openapi;
pub use k8s_openapi::api::core::v1::{Namespace, Node, Pod, Service};
pub use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

/// Serialize a resource to JSON
pub fn to_json<T: serde::Serialize>(resource: &T) -> Result<String> {
    serde_json::to_string(resource).map_err(|e| {
        ReddwarfError::serialization_error(
            format!("Failed to serialize to JSON: {}", e),
            Some(Box::new(e)),
        )
    })
}

/// Serialize a resource to pretty JSON
pub fn to_json_pretty<T: serde::Serialize>(resource: &T) -> Result<String> {
    serde_json::to_string_pretty(resource).map_err(|e| {
        ReddwarfError::serialization_error(
            format!("Failed to serialize to JSON: {}", e),
            Some(Box::new(e)),
        )
    })
}

/// Deserialize a resource from JSON
pub fn from_json<T: for<'de> serde::Deserialize<'de>>(data: &str) -> Result<T> {
    serde_json::from_str(data).map_err(|e| {
        ReddwarfError::serialization_error(
            format!("Failed to deserialize from JSON: {}", e),
            Some(Box::new(e)),
        )
    })
}

/// Serialize a resource to YAML
pub fn to_yaml<T: serde::Serialize>(resource: &T) -> Result<String> {
    serde_yaml::to_string(resource).map_err(|e| {
        ReddwarfError::serialization_error(
            format!("Failed to serialize to YAML: {}", e),
            Some(Box::new(e)),
        )
    })
}

/// Deserialize a resource from YAML
pub fn from_yaml<T: for<'de> serde::Deserialize<'de>>(data: &str) -> Result<T> {
    serde_yaml::from_str(data).map_err(|e| {
        ReddwarfError::serialization_error(
            format!("Failed to deserialize from YAML: {}", e),
            Some(Box::new(e)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_serialization() {
        let mut pod = Pod::default();
        pod.metadata.name = Some("nginx".to_string());

        let json = to_json(&pod).unwrap();
        assert!(json.contains("nginx"));

        let deserialized: Pod = from_json(&json).unwrap();
        assert_eq!(deserialized.metadata.name, Some("nginx".to_string()));
    }

    #[test]
    fn test_yaml_serialization() {
        let mut pod = Pod::default();
        pod.metadata.name = Some("nginx".to_string());

        let yaml = to_yaml(&pod).unwrap();
        assert!(yaml.contains("nginx"));

        let deserialized: Pod = from_yaml(&yaml).unwrap();
        assert_eq!(deserialized.metadata.name, Some("nginx".to_string()));
    }
}
