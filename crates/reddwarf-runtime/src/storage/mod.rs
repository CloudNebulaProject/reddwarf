mod mock;
#[cfg(target_os = "illumos")]
mod zfs;

pub use mock::MockStorageEngine;
#[cfg(target_os = "illumos")]
pub use zfs::ZfsStorageEngine;

use crate::error::Result;
use crate::types::{StoragePoolConfig, ZoneStorageOpts};
use async_trait::async_trait;

/// Information about a persistent volume
#[derive(Debug, Clone)]
pub struct VolumeInfo {
    pub name: String,
    pub dataset: String,
    pub quota: Option<String>,
}

/// Trait for pluggable storage backends
///
/// The default (and currently only real) implementation is `ZfsStorageEngine`,
/// which manages ZFS datasets for zone root filesystems, images, and
/// persistent volumes. `MockStorageEngine` provides an in-memory backend
/// for testing on non-illumos platforms.
#[async_trait]
pub trait StorageEngine: Send + Sync {
    /// Ensure all base datasets exist. Called once at startup.
    async fn initialize(&self) -> Result<()>;

    /// Create a dataset for a zone, applying per-zone options (clone_from, quota).
    async fn create_zone_dataset(&self, zone_name: &str, opts: &ZoneStorageOpts) -> Result<()>;

    /// Destroy a zone's dataset (recursive).
    async fn destroy_zone_dataset(&self, zone_name: &str) -> Result<()>;

    /// Create a ZFS snapshot.
    async fn create_snapshot(&self, dataset: &str, snapshot_name: &str) -> Result<()>;

    /// Create a persistent volume (ZFS dataset under volumes_dataset).
    async fn create_volume(&self, name: &str, quota: Option<&str>) -> Result<()>;

    /// Destroy a persistent volume.
    async fn destroy_volume(&self, name: &str) -> Result<()>;

    /// List all persistent volumes.
    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>>;

    /// Get the pool configuration.
    fn pool_config(&self) -> &StoragePoolConfig;
}
