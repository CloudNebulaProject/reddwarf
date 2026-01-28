// Allow unused assignments for diagnostic fields - they're used by the macros
#![allow(unused_assignments)]

use miette::Diagnostic;
use thiserror::Error;

/// Core error type for Reddwarf operations
#[derive(Error, Debug, Diagnostic)]
pub enum ReddwarfError {
    /// Resource not found
    #[error("Resource not found: {resource_key}")]
    #[diagnostic(
        code(reddwarf::resource_not_found),
        help("Verify the resource name, namespace, and API version are correct")
    )]
    ResourceNotFound {
        #[allow(unused)]
        resource_key: String,
    },

    /// Resource already exists
    #[error("Resource already exists: {resource_key}")]
    #[diagnostic(
        code(reddwarf::resource_already_exists),
        help("Use PUT to update existing resources, or DELETE the resource first")
    )]
    ResourceAlreadyExists {
        #[allow(unused)]
        resource_key: String,
    },

    /// Invalid resource
    #[error("Invalid resource: {reason}")]
    #[diagnostic(
        code(reddwarf::invalid_resource),
        help("{suggestion}")
    )]
    InvalidResource {
        #[allow(unused)]
        reason: String,
        #[allow(unused)]
        suggestion: String,
    },

    /// Validation failed
    #[error("Validation failed for {resource_type}: {details}")]
    #[diagnostic(
        code(reddwarf::validation_failed),
        help("{help_text}")
    )]
    ValidationFailed {
        #[allow(unused)]
        resource_type: String,
        #[allow(unused)]
        details: String,
        #[allow(unused)]
        help_text: String,
    },

    /// Conflict detected (concurrent modification)
    #[error("Conflict detected for resource {resource_key}")]
    #[diagnostic(
        code(reddwarf::conflict),
        help("This resource was modified concurrently. Resolve the conflict or retry with the latest resourceVersion")
    )]
    Conflict {
        #[allow(unused)]
        resource_key: String,
        #[allow(unused)]
        our_version: String,
        #[allow(unused)]
        their_version: String,
        #[allow(unused)]
        conflicts: Vec<String>,
    },

    /// Storage error
    #[error("Storage error: {message}")]
    #[diagnostic(
        code(reddwarf::storage_error),
        help("Check storage backend logs and ensure the data directory is accessible")
    )]
    StorageError {
        #[allow(unused)]
        message: String,
        #[source]
        #[allow(unused)]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Serialization error
    #[error("Serialization error: {message}")]
    #[diagnostic(
        code(reddwarf::serialization_error),
        help("Ensure the resource format is valid JSON or YAML")
    )]
    SerializationError {
        #[allow(unused)]
        message: String,
        #[source]
        #[allow(unused)]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Internal error
    #[error("Internal error: {message}")]
    #[diagnostic(
        code(reddwarf::internal_error),
        help("This is likely a bug. Please report it with the full error details")
    )]
    InternalError {
        #[allow(unused)]
        message: String,
    },

    /// Namespace not found
    #[error("Namespace not found: {namespace}")]
    #[diagnostic(
        code(reddwarf::namespace_not_found),
        help("Create the namespace first: kubectl create namespace {namespace}")
    )]
    NamespaceNotFound {
        #[allow(unused)]
        namespace: String,
    },

    /// Invalid API version
    #[error("Invalid API version: {api_version}")]
    #[diagnostic(
        code(reddwarf::invalid_api_version),
        help("Use a valid Kubernetes API version like 'v1' or 'apps/v1'")
    )]
    InvalidApiVersion {
        #[allow(unused)]
        api_version: String,
    },

    /// Invalid kind
    #[error("Unknown resource kind: {kind}")]
    #[diagnostic(
        code(reddwarf::invalid_kind),
        help("Supported kinds: Pod, Node, Service, Namespace, ReplicaSet, Deployment")
    )]
    InvalidKind {
        #[allow(unused)]
        kind: String,
    },
}

/// Result type alias for Reddwarf operations
pub type Result<T> = std::result::Result<T, ReddwarfError>;

impl ReddwarfError {
    /// Create a ResourceNotFound error
    pub fn resource_not_found(resource_key: impl Into<String>) -> Self {
        Self::ResourceNotFound {
            resource_key: resource_key.into(),
        }
    }

    /// Create a ResourceAlreadyExists error
    pub fn resource_already_exists(resource_key: impl Into<String>) -> Self {
        Self::ResourceAlreadyExists {
            resource_key: resource_key.into(),
        }
    }

    /// Create an InvalidResource error
    pub fn invalid_resource(reason: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self::InvalidResource {
            reason: reason.into(),
            suggestion: suggestion.into(),
        }
    }

    /// Create a ValidationFailed error
    pub fn validation_failed(
        resource_type: impl Into<String>,
        details: impl Into<String>,
        help_text: impl Into<String>,
    ) -> Self {
        Self::ValidationFailed {
            resource_type: resource_type.into(),
            details: details.into(),
            help_text: help_text.into(),
        }
    }

    /// Create a Conflict error
    pub fn conflict(
        resource_key: impl Into<String>,
        our_version: impl Into<String>,
        their_version: impl Into<String>,
        conflicts: Vec<String>,
    ) -> Self {
        Self::Conflict {
            resource_key: resource_key.into(),
            our_version: our_version.into(),
            their_version: their_version.into(),
            conflicts,
        }
    }

    /// Create a StorageError
    pub fn storage_error(
        message: impl Into<String>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::StorageError {
            message: message.into(),
            source,
        }
    }

    /// Create a SerializationError
    pub fn serialization_error(
        message: impl Into<String>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::SerializationError {
            message: message.into(),
            source,
        }
    }

    /// Create an InternalError
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::InternalError {
            message: message.into(),
        }
    }

    /// Create a NamespaceNotFound error
    pub fn namespace_not_found(namespace: impl Into<String>) -> Self {
        Self::NamespaceNotFound {
            namespace: namespace.into(),
        }
    }

    /// Create an InvalidApiVersion error
    pub fn invalid_api_version(api_version: impl Into<String>) -> Self {
        Self::InvalidApiVersion {
            api_version: api_version.into(),
        }
    }

    /// Create an InvalidKind error
    pub fn invalid_kind(kind: impl Into<String>) -> Self {
        Self::InvalidKind {
            kind: kind.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = ReddwarfError::resource_not_found("test/pod/default/nginx");
        assert!(matches!(err, ReddwarfError::ResourceNotFound { .. }));

        let err = ReddwarfError::validation_failed(
            "Pod",
            "Missing container spec",
            "Add at least one container to the pod spec",
        );
        assert!(matches!(err, ReddwarfError::ValidationFailed { .. }));
    }
}
