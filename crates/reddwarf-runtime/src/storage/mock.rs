use crate::error::Result;
use crate::storage::{StorageEngine, VolumeInfo};
use crate::types::{StoragePoolConfig, ZoneStorageOpts};
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// In-memory storage engine for testing on non-illumos platforms
///
/// Tracks dataset names in memory so tests can assert which datasets
/// were created/destroyed without touching a real ZFS pool.
pub struct MockStorageEngine {
    config: StoragePoolConfig,
    datasets: Arc<RwLock<HashSet<String>>>,
}

impl MockStorageEngine {
    pub fn new(config: StoragePoolConfig) -> Self {
        Self {
            config,
            datasets: Arc::new(RwLock::new(HashSet::new())),
        }
    }
}

#[async_trait]
impl StorageEngine for MockStorageEngine {
    async fn initialize(&self) -> Result<()> {
        let mut ds = self.datasets.write().await;
        ds.insert(self.config.zones_dataset.clone());
        ds.insert(self.config.images_dataset.clone());
        ds.insert(self.config.volumes_dataset.clone());
        debug!("Mock: initialized storage pool '{}'", self.config.pool);
        Ok(())
    }

    async fn create_zone_dataset(&self, zone_name: &str, _opts: &ZoneStorageOpts) -> Result<()> {
        let dataset = self.config.zone_dataset(zone_name);
        self.datasets.write().await.insert(dataset.clone());
        debug!("Mock: created zone dataset {}", dataset);
        Ok(())
    }

    async fn destroy_zone_dataset(&self, zone_name: &str) -> Result<()> {
        let dataset = self.config.zone_dataset(zone_name);
        self.datasets.write().await.remove(&dataset);
        debug!("Mock: destroyed zone dataset {}", dataset);
        Ok(())
    }

    async fn create_snapshot(&self, dataset: &str, snapshot_name: &str) -> Result<()> {
        let snap = format!("{}@{}", dataset, snapshot_name);
        debug!("Mock: created snapshot {}", snap);
        Ok(())
    }

    async fn create_volume(&self, name: &str, _quota: Option<&str>) -> Result<()> {
        let dataset = self.config.volume_dataset(name);
        self.datasets.write().await.insert(dataset.clone());
        debug!("Mock: created volume {}", dataset);
        Ok(())
    }

    async fn destroy_volume(&self, name: &str) -> Result<()> {
        let dataset = self.config.volume_dataset(name);
        self.datasets.write().await.remove(&dataset);
        debug!("Mock: destroyed volume {}", dataset);
        Ok(())
    }

    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>> {
        let ds = self.datasets.read().await;
        let prefix = format!("{}/", self.config.volumes_dataset);
        let volumes = ds
            .iter()
            .filter(|d| d.starts_with(&prefix))
            .map(|d| {
                let name = d.strip_prefix(&prefix).unwrap_or(d).to_string();
                VolumeInfo {
                    name,
                    dataset: d.clone(),
                    quota: None,
                }
            })
            .collect();
        Ok(volumes)
    }

    fn pool_config(&self) -> &StoragePoolConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_initialize_creates_base_datasets() {
        let config = StoragePoolConfig::from_pool("testpool");
        let engine = MockStorageEngine::new(config);

        engine.initialize().await.unwrap();

        let ds = engine.datasets.read().await;
        assert!(ds.contains("testpool/zones"));
        assert!(ds.contains("testpool/images"));
        assert!(ds.contains("testpool/volumes"));
    }

    #[tokio::test]
    async fn test_mock_zone_dataset_lifecycle() {
        let config = StoragePoolConfig::from_pool("testpool");
        let engine = MockStorageEngine::new(config);

        engine
            .create_zone_dataset("myzone", &ZoneStorageOpts::default())
            .await
            .unwrap();
        assert!(engine
            .datasets
            .read()
            .await
            .contains("testpool/zones/myzone"));

        engine.destroy_zone_dataset("myzone").await.unwrap();
        assert!(!engine
            .datasets
            .read()
            .await
            .contains("testpool/zones/myzone"));
    }

    #[tokio::test]
    async fn test_mock_volume_lifecycle() {
        let config = StoragePoolConfig::from_pool("testpool");
        let engine = MockStorageEngine::new(config);
        engine.initialize().await.unwrap();

        engine.create_volume("data-vol", None).await.unwrap();
        let vols = engine.list_volumes().await.unwrap();
        assert_eq!(vols.len(), 1);
        assert_eq!(vols[0].name, "data-vol");

        engine.destroy_volume("data-vol").await.unwrap();
        let vols = engine.list_volumes().await.unwrap();
        assert!(vols.is_empty());
    }
}
