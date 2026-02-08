use crate::error::Result;
use crate::types::{NetworkMode, ZoneConfig, ZoneInfo, ZoneState};
use async_trait::async_trait;

/// Trait for zone runtime implementations
///
/// This trait abstracts over the illumos zone lifecycle and networking
/// operations. It enables testing via `MockRuntime` on non-illumos platforms.
///
/// Storage operations (ZFS dataset create/destroy, snapshots, volumes) are
/// handled by the separate `StorageEngine` trait, which is injected into
/// runtime implementations.
#[async_trait]
pub trait ZoneRuntime: Send + Sync {
    // --- Zone lifecycle ---

    /// Create a zone configuration (zonecfg)
    async fn create_zone(&self, config: &ZoneConfig) -> Result<()>;

    /// Install a zone (zoneadm install)
    async fn install_zone(&self, zone_name: &str) -> Result<()>;

    /// Boot a zone (zoneadm boot)
    async fn boot_zone(&self, zone_name: &str) -> Result<()>;

    /// Gracefully shut down a zone (zoneadm shutdown)
    async fn shutdown_zone(&self, zone_name: &str) -> Result<()>;

    /// Forcefully halt a zone (zoneadm halt)
    async fn halt_zone(&self, zone_name: &str) -> Result<()>;

    /// Uninstall a zone (zoneadm uninstall -F)
    async fn uninstall_zone(&self, zone_name: &str) -> Result<()>;

    /// Delete a zone configuration (zonecfg delete -F)
    async fn delete_zone(&self, zone_name: &str) -> Result<()>;

    // --- Zone query ---

    /// Get the current state of a zone
    async fn get_zone_state(&self, zone_name: &str) -> Result<ZoneState>;

    /// Get full info about a zone
    async fn get_zone_info(&self, zone_name: &str) -> Result<ZoneInfo>;

    /// List all managed zones
    async fn list_zones(&self) -> Result<Vec<ZoneInfo>>;

    // --- Networking ---

    /// Set up network for a zone
    async fn setup_network(&self, zone_name: &str, network: &NetworkMode) -> Result<()>;

    /// Tear down network for a zone
    async fn teardown_network(&self, zone_name: &str, network: &NetworkMode) -> Result<()>;

    // --- High-level lifecycle ---

    /// Full provisioning: create dataset -> setup network -> create zone -> install -> boot
    async fn provision(&self, config: &ZoneConfig) -> Result<()>;

    /// Full deprovisioning: halt -> uninstall -> delete -> teardown network -> destroy dataset
    async fn deprovision(&self, config: &ZoneConfig) -> Result<()>;
}
