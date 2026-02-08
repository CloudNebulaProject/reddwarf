use crate::command::{exec, exec_unchecked};
use crate::error::{Result, RuntimeError};
use crate::storage::{StorageEngine, VolumeInfo};
use crate::types::{StoragePoolConfig, ZoneStorageOpts};
use async_trait::async_trait;
use tracing::info;

/// ZFS-backed storage engine for illumos
///
/// Manages zone root filesystems, container images, and persistent volumes
/// as ZFS datasets under the configured pool hierarchy.
pub struct ZfsStorageEngine {
    config: StoragePoolConfig,
}

impl ZfsStorageEngine {
    pub fn new(config: StoragePoolConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl StorageEngine for ZfsStorageEngine {
    async fn initialize(&self) -> Result<()> {
        info!("Initializing ZFS storage pool '{}'", self.config.pool);

        // Create base datasets (ignore already-exists errors via exec_unchecked)
        for dataset in [
            &self.config.zones_dataset,
            &self.config.images_dataset,
            &self.config.volumes_dataset,
        ] {
            let output = exec_unchecked("zfs", &["create", "-p", dataset]).await?;
            if output.exit_code != 0 && !output.stderr.contains("dataset already exists") {
                return Err(RuntimeError::StorageInitFailed {
                    pool: self.config.pool.clone(),
                    message: format!(
                        "Failed to create dataset '{}': {}",
                        dataset,
                        output.stderr.trim()
                    ),
                });
            }
        }

        info!(
            "ZFS storage initialized: zones={}, images={}, volumes={}",
            self.config.zones_dataset, self.config.images_dataset, self.config.volumes_dataset
        );
        Ok(())
    }

    async fn create_zone_dataset(&self, zone_name: &str, opts: &ZoneStorageOpts) -> Result<()> {
        let dataset = self.config.zone_dataset(zone_name);
        info!("Creating ZFS dataset for zone: {}", dataset);

        if let Some(ref clone_from) = opts.clone_from {
            exec("zfs", &["clone", clone_from, &dataset]).await?;
        } else {
            exec("zfs", &["create", &dataset]).await?;
        }

        if let Some(ref quota) = opts.quota {
            exec("zfs", &["set", &format!("quota={}", quota), &dataset]).await?;
        }

        info!("ZFS dataset created: {}", dataset);
        Ok(())
    }

    async fn destroy_zone_dataset(&self, zone_name: &str) -> Result<()> {
        let dataset = self.config.zone_dataset(zone_name);
        info!("Destroying ZFS dataset: {}", dataset);
        exec("zfs", &["destroy", "-r", &dataset]).await?;
        info!("ZFS dataset destroyed: {}", dataset);
        Ok(())
    }

    async fn create_snapshot(&self, dataset: &str, snapshot_name: &str) -> Result<()> {
        let snap = format!("{}@{}", dataset, snapshot_name);
        exec("zfs", &["snapshot", &snap]).await?;
        info!("ZFS snapshot created: {}", snap);
        Ok(())
    }

    async fn create_volume(&self, name: &str, quota: Option<&str>) -> Result<()> {
        let dataset = self.config.volume_dataset(name);
        info!("Creating persistent volume: {}", dataset);
        exec("zfs", &["create", &dataset]).await?;

        if let Some(q) = quota {
            exec("zfs", &["set", &format!("quota={}", q), &dataset]).await?;
        }

        info!("Persistent volume created: {}", dataset);
        Ok(())
    }

    async fn destroy_volume(&self, name: &str) -> Result<()> {
        let dataset = self.config.volume_dataset(name);
        info!("Destroying persistent volume: {}", dataset);
        exec("zfs", &["destroy", "-r", &dataset]).await?;
        info!("Persistent volume destroyed: {}", dataset);
        Ok(())
    }

    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>> {
        let output = exec(
            "zfs",
            &[
                "list",
                "-r",
                "-H",
                "-o",
                "name,quota",
                &self.config.volumes_dataset,
            ],
        )
        .await?;

        let mut volumes = Vec::new();
        for line in output.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            let dataset = parts[0];

            // Skip the parent dataset itself
            if dataset == self.config.volumes_dataset {
                continue;
            }

            let name = dataset
                .strip_prefix(&format!("{}/", self.config.volumes_dataset))
                .unwrap_or(dataset)
                .to_string();

            let quota = parts.get(1).and_then(|q| {
                if *q == "none" {
                    None
                } else {
                    Some(q.to_string())
                }
            });

            volumes.push(VolumeInfo {
                name,
                dataset: dataset.to_string(),
                quota,
            });
        }

        Ok(volumes)
    }

    fn pool_config(&self) -> &StoragePoolConfig {
        &self.config
    }
}
