use crate::{ApiError, Result};
use reddwarf_core::{is_valid_name, Resource};

/// Validate a resource before creation/update
pub fn validate_resource<T: Resource>(resource: &T) -> Result<()> {
    // Use the resource's validate method
    resource
        .validate()
        .map_err(|e| ApiError::ValidationFailed(e.to_string()))?;

    Ok(())
}

/// Validate a resource name (DNS-1123 subdomain)
pub fn validate_name(name: &str) -> Result<()> {
    if !is_valid_name(name) {
        return Err(ApiError::BadRequest(format!(
            "Invalid resource name: {}. Must be a valid DNS-1123 subdomain (lowercase alphanumeric, '-', or '.')",
            name
        )));
    }
    Ok(())
}

/// Validate namespace exists (for namespaced resources)
pub fn validate_namespace(namespace: &str) -> Result<()> {
    if namespace.is_empty() {
        return Err(ApiError::BadRequest(
            "Namespace cannot be empty".to_string(),
        ));
    }
    validate_name(namespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_core::Pod;

    #[test]
    fn test_validate_name() {
        assert!(validate_name("nginx").is_ok());
        assert!(validate_name("my-app").is_ok());
        assert!(validate_name("app-123").is_ok());

        assert!(validate_name("MyApp").is_err()); // uppercase
        assert!(validate_name("").is_err()); // empty
        assert!(validate_name("-app").is_err()); // starts with dash
    }

    #[test]
    fn test_validate_namespace() {
        assert!(validate_namespace("default").is_ok());
        assert!(validate_namespace("kube-system").is_ok());

        assert!(validate_namespace("").is_err());
        assert!(validate_namespace("Invalid").is_err());
    }

    #[test]
    fn test_validate_resource() {
        let mut pod = Pod::default();
        pod.metadata.name = Some("nginx".to_string());
        pod.spec = Some(Default::default());

        // Pod without containers should fail
        let result = validate_resource(&pod);
        assert!(result.is_err());
    }
}
