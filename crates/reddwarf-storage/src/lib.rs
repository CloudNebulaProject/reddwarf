//! Reddwarf Storage - Storage abstraction and redb backend
//!
//! This crate provides:
//! - KVStore trait for storage abstraction
//! - redb-based implementation
//! - Key encoding and indexing
//! - Transaction support

pub mod encoding;
pub mod error;
pub mod kv;
pub mod redb_backend;

// Re-export commonly used types
pub use encoding::{IndexKey, KeyEncoder};
pub use error::{Result, StorageError};
pub use kv::{KVStore, Transaction};
pub use redb_backend::RedbBackend;
