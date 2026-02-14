// Allow unused assignments for diagnostic fields - they're used by the thiserror/miette macros
#![allow(unused_assignments)]

pub mod api_client;
pub mod brand;
pub mod command;
pub mod controller;
pub mod error;
#[cfg(target_os = "illumos")]
pub mod illumos;
pub mod mock;
pub mod network;
pub mod node_agent;
pub mod node_health;
pub mod storage;
pub mod sysinfo;
pub mod traits;
pub mod types;
pub mod zone;

// Re-export primary types
pub use error::{Result, RuntimeError};
pub use mock::MockRuntime;
pub use network::{CidrConfig, IpAllocation, Ipam};
pub use traits::ZoneRuntime;
pub use types::{
    ContainerProcess, DirectNicConfig, EtherstubConfig, FsMount, NetworkMode, StoragePoolConfig,
    ZoneBrand, ZoneConfig, ZoneInfo, ZoneState, ZoneStorageOpts,
};

// Re-export storage types
#[cfg(target_os = "illumos")]
pub use storage::ZfsStorageEngine;
pub use storage::{MockStorageEngine, StorageEngine, VolumeInfo};

// Re-export controller and agent types
pub use api_client::ApiClient;
pub use controller::{PodController, PodControllerConfig};
pub use node_agent::{NodeAgent, NodeAgentConfig};
pub use node_health::{NodeHealthChecker, NodeHealthCheckerConfig};

// Conditionally re-export illumos runtime
#[cfg(target_os = "illumos")]
pub use illumos::IllumosRuntime;
