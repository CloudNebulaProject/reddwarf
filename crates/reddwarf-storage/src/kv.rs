use crate::Result;
use bytes::Bytes;

/// Key-value store trait
pub trait KVStore: Send + Sync {
    /// Get a value by key
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>>;

    /// Put a key-value pair
    fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key
    fn delete(&self, key: &[u8]) -> Result<()>;

    /// Scan keys with a given prefix
    fn scan(&self, prefix: &[u8]) -> Result<Vec<(Bytes, Bytes)>>;

    /// Scan keys with a given prefix and limit
    fn scan_with_limit(&self, prefix: &[u8], limit: usize) -> Result<Vec<(Bytes, Bytes)>>;

    /// Check if a key exists
    fn exists(&self, key: &[u8]) -> Result<bool>;

    /// Begin a transaction
    fn transaction(&self) -> Result<Box<dyn Transaction>>;

    /// Get all keys (use with caution on large databases)
    fn keys(&self) -> Result<Vec<Bytes>>;

    /// Get all keys with a given prefix
    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Bytes>>;
}

/// Transaction trait for atomic operations
pub trait Transaction: Send {
    /// Get a value by key
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>>;

    /// Put a key-value pair
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key
    fn delete(&mut self, key: &[u8]) -> Result<()>;

    /// Commit the transaction
    fn commit(self: Box<Self>) -> Result<()>;

    /// Rollback the transaction
    fn rollback(self: Box<Self>) -> Result<()>;
}
