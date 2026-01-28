use crate::{KVStore, Result, StorageError, Transaction as KVTransaction};
use bytes::Bytes;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info};

// Table definitions
const RESOURCES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("resources");
const JJ_METADATA_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("jj_metadata");
const INDICES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("indices");

/// redb-based storage backend
pub struct RedbBackend {
    db: Arc<Database>,
}

impl RedbBackend {
    /// Create a new RedbBackend
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        info!("Opening redb database at: {}", path.as_ref().display());

        let db = Database::create(path.as_ref()).map_err(|e| {
            StorageError::database_error(
                format!("Failed to create database: {}", e),
                Some(Box::new(e)),
            )
        })?;

        // Create tables if they don't exist
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(RESOURCES_TABLE)?;
            let _ = write_txn.open_table(JJ_METADATA_TABLE)?;
            let _ = write_txn.open_table(INDICES_TABLE)?;
        }
        write_txn.commit()?;

        info!("redb database initialized successfully");

        Ok(Self { db: Arc::new(db) })
    }

    /// Get the underlying database (for advanced operations)
    pub fn db(&self) -> Arc<Database> {
        Arc::clone(&self.db)
    }
}

impl KVStore for RedbBackend {
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        debug!("Getting key: {:?}", String::from_utf8_lossy(key));

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RESOURCES_TABLE)?;

        match table.get(key)? {
            Some(value) => {
                let bytes = value.value().to_vec();
                Ok(Some(Bytes::from(bytes)))
            }
            None => Ok(None),
        }
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        debug!("Putting key: {:?}", String::from_utf8_lossy(key));

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(RESOURCES_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;

        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        debug!("Deleting key: {:?}", String::from_utf8_lossy(key));

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(RESOURCES_TABLE)?;
            table.remove(key)?;
        }
        write_txn.commit()?;

        Ok(())
    }

    fn scan(&self, prefix: &[u8]) -> Result<Vec<(Bytes, Bytes)>> {
        debug!(
            "Scanning with prefix: {:?}",
            String::from_utf8_lossy(prefix)
        );

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RESOURCES_TABLE)?;

        let mut results = Vec::new();

        // Scan all entries
        for entry in table.iter()? {
            let (key, value) = entry?;
            let key_bytes = key.value();

            // Check if key starts with prefix
            if key_bytes.starts_with(prefix) {
                results.push((
                    Bytes::from(key_bytes.to_vec()),
                    Bytes::from(value.value().to_vec()),
                ));
            }
        }

        debug!("Scan found {} results", results.len());
        Ok(results)
    }

    fn scan_with_limit(&self, prefix: &[u8], limit: usize) -> Result<Vec<(Bytes, Bytes)>> {
        debug!(
            "Scanning with prefix: {:?}, limit: {}",
            String::from_utf8_lossy(prefix),
            limit
        );

        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RESOURCES_TABLE)?;

        let mut results = Vec::new();

        // Scan entries with limit
        for entry in table.iter()? {
            if results.len() >= limit {
                break;
            }

            let (key, value) = entry?;
            let key_bytes = key.value();

            // Check if key starts with prefix
            if key_bytes.starts_with(prefix) {
                results.push((
                    Bytes::from(key_bytes.to_vec()),
                    Bytes::from(value.value().to_vec()),
                ));
            }
        }

        debug!("Scan found {} results", results.len());
        Ok(results)
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RESOURCES_TABLE)?;
        Ok(table.get(key)?.is_some())
    }

    fn transaction(&self) -> Result<Box<dyn KVTransaction>> {
        let write_txn = self.db.begin_write()?;
        Ok(Box::new(RedbTransaction {
            txn: Some(write_txn),
            committed: false,
        }))
    }

    fn keys(&self) -> Result<Vec<Bytes>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RESOURCES_TABLE)?;

        let mut keys = Vec::new();
        for entry in table.iter()? {
            let (key, _) = entry?;
            keys.push(Bytes::from(key.value().to_vec()));
        }

        Ok(keys)
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Bytes>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RESOURCES_TABLE)?;

        let mut keys = Vec::new();
        for entry in table.iter()? {
            let (key, _) = entry?;
            let key_bytes = key.value();

            if key_bytes.starts_with(prefix) {
                keys.push(Bytes::from(key_bytes.to_vec()));
            }
        }

        Ok(keys)
    }
}

/// redb transaction implementation
struct RedbTransaction {
    txn: Option<redb::WriteTransaction>,
    committed: bool,
}

impl KVTransaction for RedbTransaction {
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        let txn = self.txn.as_ref().ok_or_else(|| {
            StorageError::transaction_error("Transaction already committed or rolled back")
        })?;

        let table = txn.open_table(RESOURCES_TABLE)?;

        let result = match table.get(key)? {
            Some(value) => {
                let bytes = value.value().to_vec();
                Some(Bytes::from(bytes))
            }
            None => None,
        };

        Ok(result)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        let txn = self.txn.as_ref().ok_or_else(|| {
            StorageError::transaction_error("Transaction already committed or rolled back")
        })?;

        let mut table = txn.open_table(RESOURCES_TABLE)?;
        table.insert(key, value)?;

        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let txn = self.txn.as_ref().ok_or_else(|| {
            StorageError::transaction_error("Transaction already committed or rolled back")
        })?;

        let mut table = txn.open_table(RESOURCES_TABLE)?;
        table.remove(key)?;

        Ok(())
    }

    fn commit(mut self: Box<Self>) -> Result<()> {
        let txn = self.txn.take().ok_or_else(|| {
            StorageError::transaction_error("Transaction already committed or rolled back")
        })?;

        txn.commit()?;
        self.committed = true;

        Ok(())
    }

    fn rollback(mut self: Box<Self>) -> Result<()> {
        let txn = self.txn.take().ok_or_else(|| {
            StorageError::transaction_error("Transaction already committed or rolled back")
        })?;

        txn.abort()?;

        Ok(())
    }
}

impl Drop for RedbTransaction {
    fn drop(&mut self) {
        if !self.committed && self.txn.is_some() {
            // Auto-rollback if not committed
            if let Some(txn) = self.txn.take() {
                let _ = txn.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_redb_backend_basic_operations() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let backend = RedbBackend::new(&db_path).unwrap();

        // Test put and get
        backend.put(b"key1", b"value1").unwrap();
        let value = backend.get(b"key1").unwrap();
        assert_eq!(value, Some(Bytes::from("value1")));

        // Test exists
        assert!(backend.exists(b"key1").unwrap());
        assert!(!backend.exists(b"key2").unwrap());

        // Test delete
        backend.delete(b"key1").unwrap();
        let value = backend.get(b"key1").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_redb_backend_scan() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let backend = RedbBackend::new(&db_path).unwrap();

        // Insert multiple keys with same prefix
        backend.put(b"prefix/key1", b"value1").unwrap();
        backend.put(b"prefix/key2", b"value2").unwrap();
        backend.put(b"other/key3", b"value3").unwrap();

        // Scan with prefix
        let results = backend.scan(b"prefix/").unwrap();
        assert_eq!(results.len(), 2);

        // Scan with limit
        let results = backend.scan_with_limit(b"prefix/", 1).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_redb_backend_transaction() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let backend = RedbBackend::new(&db_path).unwrap();

        // Test commit
        {
            let mut txn = backend.transaction().unwrap();
            txn.put(b"key1", b"value1").unwrap();
            txn.commit().unwrap();
        }

        let value = backend.get(b"key1").unwrap();
        assert_eq!(value, Some(Bytes::from("value1")));

        // Test rollback
        {
            let mut txn = backend.transaction().unwrap();
            txn.put(b"key2", b"value2").unwrap();
            txn.rollback().unwrap();
        }

        let value = backend.get(b"key2").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_redb_backend_keys() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let backend = RedbBackend::new(&db_path).unwrap();

        backend.put(b"key1", b"value1").unwrap();
        backend.put(b"key2", b"value2").unwrap();
        backend.put(b"prefix/key3", b"value3").unwrap();

        // Test keys
        let keys = backend.keys().unwrap();
        assert_eq!(keys.len(), 3);

        // Test keys_with_prefix
        let keys = backend.keys_with_prefix(b"prefix/").unwrap();
        assert_eq!(keys.len(), 1);
    }
}
