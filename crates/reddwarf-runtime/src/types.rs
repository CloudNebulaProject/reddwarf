use serde::{Deserialize, Serialize};

/// Zone brand type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneBrand {
    /// LX branded zone (Linux emulation)
    Lx,
    /// Custom reddwarf brand (Pod = Zone, containers = supervised processes)
    Reddwarf,
}

impl ZoneBrand {
    /// Get the zonecfg brand string
    pub fn as_str(&self) -> &'static str {
        match self {
            ZoneBrand::Lx => "lx",
            ZoneBrand::Reddwarf => "reddwarf",
        }
    }
}

impl std::fmt::Display for ZoneBrand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Zone lifecycle state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneState {
    Configured,
    Incomplete,
    Installed,
    Ready,
    Running,
    ShuttingDown,
    Down,
    Absent,
}

impl ZoneState {
    /// Map zone state to Kubernetes Pod phase
    pub fn to_pod_phase(&self) -> &'static str {
        match self {
            ZoneState::Configured => "Pending",
            ZoneState::Incomplete => "Pending",
            ZoneState::Installed => "Pending",
            ZoneState::Ready => "Pending",
            ZoneState::Running => "Running",
            ZoneState::ShuttingDown => "Succeeded",
            ZoneState::Down => "Failed",
            ZoneState::Absent => "Unknown",
        }
    }

    /// Parse from zoneadm output string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "configured" => Some(ZoneState::Configured),
            "incomplete" => Some(ZoneState::Incomplete),
            "installed" => Some(ZoneState::Installed),
            "ready" => Some(ZoneState::Ready),
            "running" => Some(ZoneState::Running),
            "shutting_down" => Some(ZoneState::ShuttingDown),
            "down" => Some(ZoneState::Down),
            _ => None,
        }
    }
}

impl std::fmt::Display for ZoneState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ZoneState::Configured => "configured",
            ZoneState::Incomplete => "incomplete",
            ZoneState::Installed => "installed",
            ZoneState::Ready => "ready",
            ZoneState::Running => "running",
            ZoneState::ShuttingDown => "shutting_down",
            ZoneState::Down => "down",
            ZoneState::Absent => "absent",
        };
        write!(f, "{}", s)
    }
}

/// Network mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMode {
    /// Isolated overlay via etherstub + VNIC
    Etherstub(EtherstubConfig),
    /// Direct VNIC on physical NIC
    Direct(DirectNicConfig),
}

/// Etherstub-based network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtherstubConfig {
    /// Name of the etherstub to create/use
    pub etherstub_name: String,
    /// Name of the VNIC on the etherstub
    pub vnic_name: String,
    /// IP address to assign
    pub ip_address: String,
    /// Gateway address
    pub gateway: String,
}

/// Direct NIC-based network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectNicConfig {
    /// Physical NIC to create the VNIC on
    pub physical_nic: String,
    /// Name of the VNIC
    pub vnic_name: String,
    /// IP address to assign
    pub ip_address: String,
    /// Gateway address
    pub gateway: String,
}

/// ZFS dataset configuration for zone storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZfsConfig {
    /// Parent dataset (e.g., "rpool/zones")
    pub parent_dataset: String,
    /// Optional snapshot to clone from (fast provisioning)
    pub clone_from: Option<String>,
    /// Optional quota
    pub quota: Option<String>,
}

/// A supervised process within a zone (for reddwarf brand)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerProcess {
    /// Process name (maps to container name)
    pub name: String,
    /// Command and arguments
    pub command: Vec<String>,
    /// Working directory
    pub working_dir: Option<String>,
    /// Environment variables
    pub env: Vec<(String, String)>,
}

/// Filesystem mount specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsMount {
    /// Source path on the global zone
    pub source: String,
    /// Mount point inside the zone
    pub mountpoint: String,
    /// Filesystem type (e.g., "lofs")
    pub fs_type: String,
    /// Mount options
    pub options: Vec<String>,
}

/// Complete zone configuration for provisioning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneConfig {
    /// Zone name (must be unique on the host)
    pub zone_name: String,
    /// Zone brand
    pub brand: ZoneBrand,
    /// Zone root path
    pub zonepath: String,
    /// Network configuration
    pub network: NetworkMode,
    /// ZFS dataset configuration
    pub zfs: ZfsConfig,
    /// LX brand image path (only for Lx brand)
    pub lx_image_path: Option<String>,
    /// Supervised processes (for reddwarf brand)
    pub processes: Vec<ContainerProcess>,
    /// CPU cap (fraction, e.g., "1.0" = 1 CPU)
    pub cpu_cap: Option<String>,
    /// Memory cap (e.g., "512M", "2G")
    pub memory_cap: Option<String>,
    /// Additional filesystem mounts
    pub fs_mounts: Vec<FsMount>,
}

/// Information about an existing zone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneInfo {
    /// Zone name
    pub zone_name: String,
    /// Zone numeric ID (assigned when running)
    pub zone_id: Option<i32>,
    /// Current state
    pub state: ZoneState,
    /// Zone root path
    pub zonepath: String,
    /// Zone brand
    pub brand: String,
    /// Zone UUID
    pub uuid: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_state_to_pod_phase() {
        assert_eq!(ZoneState::Configured.to_pod_phase(), "Pending");
        assert_eq!(ZoneState::Incomplete.to_pod_phase(), "Pending");
        assert_eq!(ZoneState::Installed.to_pod_phase(), "Pending");
        assert_eq!(ZoneState::Ready.to_pod_phase(), "Pending");
        assert_eq!(ZoneState::Running.to_pod_phase(), "Running");
        assert_eq!(ZoneState::ShuttingDown.to_pod_phase(), "Succeeded");
        assert_eq!(ZoneState::Down.to_pod_phase(), "Failed");
        assert_eq!(ZoneState::Absent.to_pod_phase(), "Unknown");
    }

    #[test]
    fn test_zone_brand_display() {
        assert_eq!(ZoneBrand::Lx.as_str(), "lx");
        assert_eq!(ZoneBrand::Reddwarf.as_str(), "reddwarf");
    }

    #[test]
    fn test_zone_state_from_str() {
        assert_eq!(ZoneState::parse("running"), Some(ZoneState::Running));
        assert_eq!(ZoneState::parse("installed"), Some(ZoneState::Installed));
        assert_eq!(ZoneState::parse("configured"), Some(ZoneState::Configured));
        assert_eq!(ZoneState::parse("bogus"), None);
    }
}
