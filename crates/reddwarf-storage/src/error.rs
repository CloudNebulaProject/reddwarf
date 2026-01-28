// Allow unused assignments for diagnostic fields - they're used by the macros
#![allow(unused_assignments)]

use miette::Diagnostic;
use thiserror::Error;

/// Storage error type
#[derive(Error, Debug, Diagnostic)]
pub enum StorageError {
    /// Key not found
    #[error("Key not found: {key}")]
    #[diagnostic(
        code(storage::key_not_found),
        help("Verify the key exists in the database")
    )]
    KeyNotFound {
        key: String,
    },

    /// Database error
    #[error("Database error: {message}")]
    #[diagnostic(
        code(storage::database_error),
        help("Check database logs and ensure the data directory is accessible and not corrupted")
    )]
    DatabaseError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Transaction error
    #[error("Transaction error: {message}")]
    #[diagnostic(
        code(storage::transaction_error),
        help("Ensure the transaction is not already committed or aborted")
    )]
    TransactionError {
        message: String,
    },

    /// Serialization error
    #[error("Serialization error: {message}")]
    #[diagnostic(
        code(storage::serialization_error),
        help("Ensure the data is valid and can be serialized")
    )]
    SerializationError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// I/O error
    #[error("I/O error: {message}")]
    #[diagnostic(
        code(storage::io_error),
        help("Check filesystem permissions and available disk space")
    )]
    IoError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

/// Result type for storage operations
pub type Result<T> = std::result::Result<T, StorageError>;

impl StorageError {
    /// Create a KeyNotFound error
    pub fn key_not_found(key: impl Into<String>) -> Self {
        Self::KeyNotFound { key: key.into() }
    }

    /// Create a DatabaseError
    pub fn database_error(
        message: impl Into<String>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::DatabaseError {
            message: message.into(),
            source,
        }
    }

    /// Create a TransactionError
    pub fn transaction_error(message: impl Into<String>) -> Self {
        Self::TransactionError {
            message: message.into(),
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

    /// Create an IoError
    pub fn io_error(
        message: impl Into<String>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::IoError {
            message: message.into(),
            source,
        }
    }
}

impl From<redb::Error> for StorageError {
    fn from(err: redb::Error) -> Self {
        match err {
            redb::Error::TableDoesNotExist(_) => {
                StorageError::database_error("Table does not exist", Some(Box::new(err)))
            }
            _ => StorageError::database_error(format!("redb error: {}", err), Some(Box::new(err))),
        }
    }
}

impl From<redb::TransactionError> for StorageError {
    fn from(err: redb::TransactionError) -> Self {
        StorageError::transaction_error(format!("Transaction error: {}", err))
    }
}

impl From<redb::StorageError> for StorageError {
    fn from(err: redb::StorageError) -> Self {
        StorageError::database_error(format!("Storage error: {}", err), Some(Box::new(err)))
    }
}

impl From<redb::TableError> for StorageError {
    fn from(err: redb::TableError) -> Self {
        StorageError::database_error(format!("Table error: {}", err), Some(Box::new(err)))
    }
}

impl From<redb::CommitError> for StorageError {
    fn from(err: redb::CommitError) -> Self {
        StorageError::transaction_error(format!("Commit error: {}", err))
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(err: serde_json::Error) -> Self {
        StorageError::serialization_error(format!("JSON error: {}", err), Some(Box::new(err)))
    }
}

impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        StorageError::io_error(format!("I/O error: {}", err), Some(Box::new(err)))
    }
}
