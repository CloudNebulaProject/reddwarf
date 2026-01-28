// Allow unused assignments for diagnostic fields - they're used by the macros
#![allow(unused_assignments)]

use miette::Diagnostic;
use thiserror::Error;

/// Scheduler error type
#[derive(Error, Debug, Diagnostic)]
pub enum SchedulerError {
    /// No suitable nodes found
    #[error("No suitable nodes found for pod {pod_name}")]
    #[diagnostic(
        code(scheduler::no_suitable_nodes),
        help("Check node resources, taints, and pod requirements")
    )]
    NoSuitableNodes {
        pod_name: String,
        reason: String,
    },

    /// Scheduling failed
    #[error("Scheduling failed: {message}")]
    #[diagnostic(
        code(scheduler::scheduling_failed),
        help("{suggestion}")
    )]
    SchedulingFailed {
        message: String,
        suggestion: String,
    },

    /// Storage error
    #[error("Storage error: {0}")]
    #[diagnostic(
        code(scheduler::storage_error),
        help("Check the underlying storage system")
    )]
    StorageError(#[from] reddwarf_storage::StorageError),

    /// Core error
    #[error("Core error: {0}")]
    #[diagnostic(
        code(scheduler::core_error),
        help("This is an internal error")
    )]
    CoreError(#[from] reddwarf_core::ReddwarfError),

    /// Internal error
    #[error("Internal error: {message}")]
    #[diagnostic(
        code(scheduler::internal_error),
        help("This is likely a bug. Please report it")
    )]
    InternalError {
        message: String,
    },
}

/// Result type for scheduler operations
pub type Result<T> = std::result::Result<T, SchedulerError>;

impl SchedulerError {
    /// Create a NoSuitableNodes error
    pub fn no_suitable_nodes(pod_name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::NoSuitableNodes {
            pod_name: pod_name.into(),
            reason: reason.into(),
        }
    }

    /// Create a SchedulingFailed error
    pub fn scheduling_failed(message: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self::SchedulingFailed {
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
