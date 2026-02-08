use crate::brand::lx::lx_install_args;
use crate::command::exec;
use crate::error::Result;
use crate::storage::StorageEngine;
use crate::traits::ZoneRuntime;
use crate::types::*;
use crate::zone::config::generate_zonecfg;
use crate::zone::state::parse_zoneadm_line;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

/// illumos zone runtime implementation
///
/// Manages real zones via zonecfg/zoneadm, dladm for networking.
/// Storage (ZFS datasets) is delegated to the injected `StorageEngine`.
pub struct IllumosRuntime {
    storage: Arc<dyn StorageEngine>,
}

impl IllumosRuntime {
    pub fn new(storage: Arc<dyn StorageEngine>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl ZoneRuntime for IllumosRuntime {
    async fn create_zone(&self, config: &ZoneConfig) -> Result<()> {
        info!("Creating zone: {}", config.zone_name);

        let zonecfg_content = generate_zonecfg(config)?;

        // Write config to a temp file, then apply via zonecfg
        let tmp_path = format!("/tmp/zonecfg-{}.cmd", config.zone_name);
        tokio::fs::write(&tmp_path, &zonecfg_content)
            .await
            .map_err(|e| {
                crate::error::RuntimeError::zone_operation_failed(&config.zone_name, e.to_string())
            })?;

        let result = exec("zonecfg", &["-z", &config.zone_name, "-f", &tmp_path]).await;

        // Clean up temp file (best-effort)
        let _ = tokio::fs::remove_file(&tmp_path).await;

        result?;
        info!("Zone configured: {}", config.zone_name);
        Ok(())
    }

    async fn install_zone(&self, zone_name: &str) -> Result<()> {
        info!("Installing zone: {}", zone_name);
        exec("zoneadm", &["-z", zone_name, "install"]).await?;
        info!("Zone installed: {}", zone_name);
        Ok(())
    }

    async fn boot_zone(&self, zone_name: &str) -> Result<()> {
        info!("Booting zone: {}", zone_name);
        exec("zoneadm", &["-z", zone_name, "boot"]).await?;
        info!("Zone booted: {}", zone_name);
        Ok(())
    }

    async fn shutdown_zone(&self, zone_name: &str) -> Result<()> {
        info!("Shutting down zone: {}", zone_name);
        exec("zoneadm", &["-z", zone_name, "shutdown"]).await?;
        info!("Zone shutdown: {}", zone_name);
        Ok(())
    }

    async fn halt_zone(&self, zone_name: &str) -> Result<()> {
        info!("Halting zone: {}", zone_name);
        exec("zoneadm", &["-z", zone_name, "halt"]).await?;
        info!("Zone halted: {}", zone_name);
        Ok(())
    }

    async fn uninstall_zone(&self, zone_name: &str) -> Result<()> {
        info!("Uninstalling zone: {}", zone_name);
        exec("zoneadm", &["-z", zone_name, "uninstall", "-F"]).await?;
        info!("Zone uninstalled: {}", zone_name);
        Ok(())
    }

    async fn delete_zone(&self, zone_name: &str) -> Result<()> {
        info!("Deleting zone: {}", zone_name);
        exec("zonecfg", &["-z", zone_name, "delete", "-F"]).await?;
        info!("Zone deleted: {}", zone_name);
        Ok(())
    }

    async fn get_zone_state(&self, zone_name: &str) -> Result<ZoneState> {
        let output = exec("zoneadm", &["-z", zone_name, "list", "-p"]).await?;
        let line = output.stdout.trim();
        let info = parse_zoneadm_line(line)?;
        Ok(info.state)
    }

    async fn get_zone_info(&self, zone_name: &str) -> Result<ZoneInfo> {
        let output = exec("zoneadm", &["-z", zone_name, "list", "-cp"]).await?;
        let line = output.stdout.trim();
        parse_zoneadm_line(line)
    }

    async fn list_zones(&self) -> Result<Vec<ZoneInfo>> {
        let output = exec("zoneadm", &["list", "-cp"]).await?;
        let mut zones = Vec::new();

        for line in output.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let info = parse_zoneadm_line(line)?;
            // Filter out the global zone
            if info.zone_name == "global" {
                continue;
            }
            zones.push(info);
        }

        Ok(zones)
    }

    async fn setup_network(&self, zone_name: &str, network: &NetworkMode) -> Result<()> {
        info!("Setting up network for zone: {}", zone_name);

        match network {
            NetworkMode::Etherstub(cfg) => {
                // Create etherstub (ignore if already exists)
                let _ = exec("dladm", &["create-etherstub", &cfg.etherstub_name]).await;
                // Create VNIC on etherstub
                exec(
                    "dladm",
                    &["create-vnic", "-l", &cfg.etherstub_name, &cfg.vnic_name],
                )
                .await?;
            }
            NetworkMode::Direct(cfg) => {
                // Create VNIC directly on physical NIC
                exec(
                    "dladm",
                    &["create-vnic", "-l", &cfg.physical_nic, &cfg.vnic_name],
                )
                .await?;
            }
        }

        info!("Network setup complete for zone: {}", zone_name);
        Ok(())
    }

    async fn teardown_network(&self, zone_name: &str, network: &NetworkMode) -> Result<()> {
        info!("Tearing down network for zone: {}", zone_name);

        let vnic_name = match network {
            NetworkMode::Etherstub(cfg) => &cfg.vnic_name,
            NetworkMode::Direct(cfg) => &cfg.vnic_name,
        };

        exec("dladm", &["delete-vnic", vnic_name]).await?;

        info!("Network teardown complete for zone: {}", zone_name);
        Ok(())
    }

    async fn provision(&self, config: &ZoneConfig) -> Result<()> {
        info!("Provisioning zone: {}", config.zone_name);

        self.storage
            .create_zone_dataset(&config.zone_name, &config.storage)
            .await?;
        self.setup_network(&config.zone_name, &config.network)
            .await?;
        self.create_zone(config).await?;

        // LX brand needs image path for install
        if config.brand == ZoneBrand::Lx {
            let args = lx_install_args(config)?;
            let mut cmd_args = vec!["-z", &config.zone_name, "install"];
            let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            cmd_args.extend(str_args);
            exec("zoneadm", &cmd_args).await?;
        } else {
            self.install_zone(&config.zone_name).await?;
        }

        self.boot_zone(&config.zone_name).await?;

        info!("Zone provisioned: {}", config.zone_name);
        Ok(())
    }

    async fn deprovision(&self, config: &ZoneConfig) -> Result<()> {
        info!("Deprovisioning zone: {}", config.zone_name);

        // Best-effort halt (may fail if already not running)
        let _ = self.halt_zone(&config.zone_name).await;
        self.uninstall_zone(&config.zone_name).await?;
        self.delete_zone(&config.zone_name).await?;
        self.teardown_network(&config.zone_name, &config.network)
            .await?;
        self.storage.destroy_zone_dataset(&config.zone_name).await?;

        info!("Zone deprovisioned: {}", config.zone_name);
        Ok(())
    }
}
