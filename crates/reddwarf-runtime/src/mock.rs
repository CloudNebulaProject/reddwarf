use crate::error::{Result, RuntimeError};
use crate::storage::StorageEngine;
use crate::traits::ZoneRuntime;
use crate::types::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// In-memory zone state for MockRuntime
#[derive(Debug, Clone)]
struct MockZone {
    config: ZoneConfig,
    state: ZoneState,
    zone_id: Option<i32>,
}

/// Mock runtime for testing on non-illumos platforms
///
/// Maintains an in-memory zone registry and simulates state transitions.
/// All network operations are no-ops. Storage operations are delegated to
/// the injected `StorageEngine`.
pub struct MockRuntime {
    zones: Arc<RwLock<HashMap<String, MockZone>>>,
    next_id: Arc<RwLock<i32>>,
    storage: Arc<dyn StorageEngine>,
}

impl MockRuntime {
    pub fn new(storage: Arc<dyn StorageEngine>) -> Self {
        Self {
            zones: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(1)),
            storage,
        }
    }
}

#[async_trait]
impl ZoneRuntime for MockRuntime {
    async fn create_zone(&self, config: &ZoneConfig) -> Result<()> {
        let mut zones = self.zones.write().await;
        if zones.contains_key(&config.zone_name) {
            return Err(RuntimeError::zone_already_exists(&config.zone_name));
        }
        zones.insert(
            config.zone_name.clone(),
            MockZone {
                config: config.clone(),
                state: ZoneState::Configured,
                zone_id: None,
            },
        );
        debug!("Mock: zone created: {}", config.zone_name);
        Ok(())
    }

    async fn install_zone(&self, zone_name: &str) -> Result<()> {
        let mut zones = self.zones.write().await;
        let zone = zones
            .get_mut(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;

        if zone.state != ZoneState::Configured {
            return Err(RuntimeError::invalid_state_transition(
                zone_name,
                zone.state.to_string(),
                "installed",
                "configured",
            ));
        }

        zone.state = ZoneState::Installed;
        debug!("Mock: zone installed: {}", zone_name);
        Ok(())
    }

    async fn boot_zone(&self, zone_name: &str) -> Result<()> {
        let mut zones = self.zones.write().await;
        let zone = zones
            .get_mut(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;

        if zone.state != ZoneState::Installed && zone.state != ZoneState::Ready {
            return Err(RuntimeError::invalid_state_transition(
                zone_name,
                zone.state.to_string(),
                "running",
                "installed or ready",
            ));
        }

        let mut next_id = self.next_id.write().await;
        zone.zone_id = Some(*next_id);
        *next_id += 1;
        zone.state = ZoneState::Running;
        debug!("Mock: zone booted: {}", zone_name);
        Ok(())
    }

    async fn shutdown_zone(&self, zone_name: &str) -> Result<()> {
        let mut zones = self.zones.write().await;
        let zone = zones
            .get_mut(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;

        if zone.state != ZoneState::Running {
            return Err(RuntimeError::invalid_state_transition(
                zone_name,
                zone.state.to_string(),
                "installed",
                "running",
            ));
        }

        zone.state = ZoneState::Installed;
        zone.zone_id = None;
        debug!("Mock: zone shut down: {}", zone_name);
        Ok(())
    }

    async fn halt_zone(&self, zone_name: &str) -> Result<()> {
        let mut zones = self.zones.write().await;
        let zone = zones
            .get_mut(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;

        if zone.state != ZoneState::Running {
            return Err(RuntimeError::invalid_state_transition(
                zone_name,
                zone.state.to_string(),
                "installed",
                "running",
            ));
        }

        zone.state = ZoneState::Installed;
        zone.zone_id = None;
        debug!("Mock: zone halted: {}", zone_name);
        Ok(())
    }

    async fn uninstall_zone(&self, zone_name: &str) -> Result<()> {
        let mut zones = self.zones.write().await;
        let zone = zones
            .get_mut(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;

        if zone.state != ZoneState::Installed {
            return Err(RuntimeError::invalid_state_transition(
                zone_name,
                zone.state.to_string(),
                "configured",
                "installed",
            ));
        }

        zone.state = ZoneState::Configured;
        debug!("Mock: zone uninstalled: {}", zone_name);
        Ok(())
    }

    async fn delete_zone(&self, zone_name: &str) -> Result<()> {
        let mut zones = self.zones.write().await;
        let zone = zones
            .get(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;

        if zone.state != ZoneState::Configured {
            return Err(RuntimeError::invalid_state_transition(
                zone_name,
                zone.state.to_string(),
                "absent",
                "configured",
            ));
        }

        zones.remove(zone_name);
        debug!("Mock: zone deleted: {}", zone_name);
        Ok(())
    }

    async fn get_zone_state(&self, zone_name: &str) -> Result<ZoneState> {
        let zones = self.zones.read().await;
        let zone = zones
            .get(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;
        Ok(zone.state.clone())
    }

    async fn get_zone_info(&self, zone_name: &str) -> Result<ZoneInfo> {
        let zones = self.zones.read().await;
        let zone = zones
            .get(zone_name)
            .ok_or_else(|| RuntimeError::zone_not_found(zone_name))?;

        Ok(ZoneInfo {
            zone_name: zone_name.to_string(),
            zone_id: zone.zone_id,
            state: zone.state.clone(),
            zonepath: zone.config.zonepath.clone(),
            brand: zone.config.brand.to_string(),
            uuid: String::new(),
        })
    }

    async fn list_zones(&self) -> Result<Vec<ZoneInfo>> {
        let zones = self.zones.read().await;
        let mut infos = Vec::new();

        for (name, zone) in zones.iter() {
            infos.push(ZoneInfo {
                zone_name: name.clone(),
                zone_id: zone.zone_id,
                state: zone.state.clone(),
                zonepath: zone.config.zonepath.clone(),
                brand: zone.config.brand.to_string(),
                uuid: String::new(),
            });
        }

        Ok(infos)
    }

    async fn setup_network(&self, zone_name: &str, _network: &NetworkMode) -> Result<()> {
        debug!("Mock: network setup for zone: {}", zone_name);
        Ok(())
    }

    async fn teardown_network(&self, zone_name: &str, _network: &NetworkMode) -> Result<()> {
        debug!("Mock: network teardown for zone: {}", zone_name);
        Ok(())
    }

    async fn provision(&self, config: &ZoneConfig) -> Result<()> {
        self.storage
            .create_zone_dataset(&config.zone_name, &config.storage)
            .await?;
        self.setup_network(&config.zone_name, &config.network)
            .await?;
        self.create_zone(config).await?;
        self.install_zone(&config.zone_name).await?;
        self.boot_zone(&config.zone_name).await?;
        Ok(())
    }

    async fn deprovision(&self, config: &ZoneConfig) -> Result<()> {
        // Get current state to determine deprovision path
        let state = {
            let zones = self.zones.read().await;
            zones.get(&config.zone_name).map(|z| z.state.clone())
        };

        match state {
            Some(ZoneState::Running) => {
                self.halt_zone(&config.zone_name).await?;
                self.uninstall_zone(&config.zone_name).await?;
                self.delete_zone(&config.zone_name).await?;
            }
            Some(ZoneState::Installed) => {
                self.uninstall_zone(&config.zone_name).await?;
                self.delete_zone(&config.zone_name).await?;
            }
            Some(ZoneState::Configured) => {
                self.delete_zone(&config.zone_name).await?;
            }
            Some(_) => {
                return Err(RuntimeError::zone_operation_failed(
                    &config.zone_name,
                    "Zone is in an unexpected state for deprovisioning",
                ));
            }
            None => {
                return Err(RuntimeError::zone_not_found(&config.zone_name));
            }
        }

        self.teardown_network(&config.zone_name, &config.network)
            .await?;
        self.storage.destroy_zone_dataset(&config.zone_name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MockStorageEngine;
    use crate::types::StoragePoolConfig;

    fn make_test_storage() -> Arc<dyn StorageEngine> {
        Arc::new(MockStorageEngine::new(StoragePoolConfig::from_pool(
            "rpool",
        )))
    }

    fn make_test_config(name: &str) -> ZoneConfig {
        ZoneConfig {
            zone_name: name.to_string(),
            brand: ZoneBrand::Reddwarf,
            zonepath: format!("/zones/{}", name),
            network: NetworkMode::Etherstub(EtherstubConfig {
                etherstub_name: "reddwarf0".to_string(),
                vnic_name: format!("vnic_{}", name),
                ip_address: "10.0.0.2".to_string(),
                gateway: "10.0.0.1".to_string(),
                prefix_len: 16,
            }),
            storage: ZoneStorageOpts::default(),
            lx_image_path: None,
            processes: vec![],
            cpu_cap: None,
            memory_cap: None,
            fs_mounts: vec![],
        }
    }

    #[tokio::test]
    async fn test_provision_transitions_to_running() {
        let rt = MockRuntime::new(make_test_storage());
        let config = make_test_config("test-zone");

        rt.provision(&config).await.unwrap();

        let state = rt.get_zone_state("test-zone").await.unwrap();
        assert_eq!(state, ZoneState::Running);
    }

    #[tokio::test]
    async fn test_deprovision_removes_zone() {
        let rt = MockRuntime::new(make_test_storage());
        let config = make_test_config("test-zone");

        rt.provision(&config).await.unwrap();
        rt.deprovision(&config).await.unwrap();

        let result = rt.get_zone_state("test-zone").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_duplicate_create_zone_returns_error() {
        let rt = MockRuntime::new(make_test_storage());
        let config = make_test_config("test-zone");

        rt.create_zone(&config).await.unwrap();
        let result = rt.create_zone(&config).await;
        assert!(matches!(
            result.unwrap_err(),
            RuntimeError::ZoneAlreadyExists { .. }
        ));
    }

    #[tokio::test]
    async fn test_ops_on_missing_zone_return_not_found() {
        let rt = MockRuntime::new(make_test_storage());

        assert!(matches!(
            rt.get_zone_state("nonexistent").await.unwrap_err(),
            RuntimeError::ZoneNotFound { .. }
        ));
        assert!(matches!(
            rt.boot_zone("nonexistent").await.unwrap_err(),
            RuntimeError::ZoneNotFound { .. }
        ));
        assert!(matches!(
            rt.halt_zone("nonexistent").await.unwrap_err(),
            RuntimeError::ZoneNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_list_zones_returns_all_provisioned() {
        let rt = MockRuntime::new(make_test_storage());

        for i in 0..3 {
            let config = make_test_config(&format!("zone-{}", i));
            rt.provision(&config).await.unwrap();
        }

        let zones = rt.list_zones().await.unwrap();
        assert_eq!(zones.len(), 3);
    }

    #[tokio::test]
    async fn test_zone_info() {
        let rt = MockRuntime::new(make_test_storage());
        let config = make_test_config("info-zone");

        rt.provision(&config).await.unwrap();

        let info = rt.get_zone_info("info-zone").await.unwrap();
        assert_eq!(info.zone_name, "info-zone");
        assert_eq!(info.state, ZoneState::Running);
        assert_eq!(info.zonepath, "/zones/info-zone");
        assert_eq!(info.brand, "reddwarf");
        assert!(info.zone_id.is_some());
    }
}
