//! Reddwarf Storage - Storage abstraction and redb backend
//!
//! This crate provides:
//! - KVStore trait for storage abstraction
//! - redb-based implementation
//! - Key encoding and indexing
//! - Transaction support

pub mod error;
pub mod kv;
pub mod redb_backend;
pub mod encoding;

// Re-export commonly used types
pub use error::{StorageError, Result};
pub use kv::{KVStore, Transaction};
pub use redb_backend::RedbBackend;
pub use encoding::{KeyEncoder, IndexKey};
