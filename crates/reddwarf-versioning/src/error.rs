// Allow unused assignments for diagnostic fields - they're used by the macros
#![allow(unused_assignments)]

use miette::Diagnostic;
use thiserror::Error;

/// Versioning error type
#[derive(Error, Debug, Diagnostic)]
pub enum VersioningError {
    /// Commit not found
    #[error("Commit not found: {commit_id}")]
    #[diagnostic(
        code(versioning::commit_not_found),
        help("Verify the commit ID is correct and exists in the repository")
    )]
    CommitNotFound { commit_id: String },

    /// Conflict detected
    #[error("Conflict detected: {message}")]
    #[diagnostic(
        code(versioning::conflict),
        help("Resolve the conflict by choosing one side or manually merging the changes")
    )]
    Conflict {
        message: String,
        conflicts: Vec<String>,
    },

    /// Invalid operation
    #[error("Invalid operation: {message}")]
    #[diagnostic(code(versioning::invalid_operation), help("{suggestion}"))]
    InvalidOperation { message: String, suggestion: String },

    /// Storage error
    #[error("Storage error: {0}")]
    #[diagnostic(
        code(versioning::storage_error),
        help("Check the underlying storage system")
    )]
    StorageError(#[from] reddwarf_storage::StorageError),

    /// Core error
    #[error("Core error: {0}")]
    #[diagnostic(code(versioning::core_error), help("This is an internal error"))]
    CoreError(#[from] reddwarf_core::ReddwarfError),

    /// Internal error
    #[error("Internal error: {message}")]
    #[diagnostic(
        code(versioning::internal_error),
        help("This is likely a bug. Please report it with full error details")
    )]
    InternalError { message: String },
}

/// Result type for versioning operations
pub type Result<T> = std::result::Result<T, VersioningError>;

impl VersioningError {
    /// Create a CommitNotFound error
    pub fn commit_not_found(commit_id: impl Into<String>) -> Self {
        Self::CommitNotFound {
            commit_id: commit_id.into(),
        }
    }

    /// Create a Conflict error
    pub fn conflict(message: impl Into<String>, conflicts: Vec<String>) -> Self {
        Self::Conflict {
            message: message.into(),
            conflicts,
        }
    }

    /// Create an InvalidOperation error
    pub fn invalid_operation(message: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self::InvalidOperation {
            message: message.into(),
            suggestion: suggestion.into(),
        }
    }

    /// Create an InternalError
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::InternalError {
            message: message.into(),
        }
    }
}
