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
pub mod traits;
pub mod types;
pub mod zfs;
pub mod zone;

// Re-export primary types
pub use error::{Result, RuntimeError};
pub use mock::MockRuntime;
pub use traits::ZoneRuntime;
pub use types::{
    ContainerProcess, DirectNicConfig, EtherstubConfig, FsMount, NetworkMode, ZfsConfig, ZoneBrand,
    ZoneConfig, ZoneInfo, ZoneState,
};

// Re-export controller and agent types
pub use api_client::ApiClient;
pub use controller::{PodController, PodControllerConfig};
pub use node_agent::{NodeAgent, NodeAgentConfig};

// Conditionally re-export illumos runtime
#[cfg(target_os = "illumos")]
pub use illumos::IllumosRuntime;
