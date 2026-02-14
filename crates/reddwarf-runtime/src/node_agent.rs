use crate::api_client::ApiClient;
use crate::error::{Result, RuntimeError};
use crate::sysinfo::{
    compute_node_resources, format_memory_quantity, NodeResources, ResourceReservation,
};
use k8s_openapi::api::core::v1::{Node, NodeAddress, NodeCondition, NodeStatus};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Configuration for the node agent
#[derive(Debug, Clone)]
pub struct NodeAgentConfig {
    /// Name to register this node as
    pub node_name: String,
    /// API server URL
    pub api_url: String,
    /// Interval between heartbeats
    pub heartbeat_interval: Duration,
    /// CPU to reserve for system daemons, in millicores (default: 100 = 100m)
    pub system_reserved_cpu_millicores: i64,
    /// Memory to reserve for system daemons, in bytes (default: 256Mi)
    pub system_reserved_memory_bytes: i64,
    /// Maximum number of pods this node will accept (default: 110)
    pub max_pods: u32,
}

impl NodeAgentConfig {
    pub fn new(node_name: String, api_url: String) -> Self {
        Self {
            node_name,
            api_url,
            heartbeat_interval: Duration::from_secs(10),
            system_reserved_cpu_millicores: 100,
            system_reserved_memory_bytes: 256 * 1024 * 1024,
            max_pods: 110,
        }
    }
}

/// Node agent that registers this host as a Node and sends periodic heartbeats
pub struct NodeAgent {
    api_client: Arc<ApiClient>,
    config: NodeAgentConfig,
    /// Detected system resources (None if detection failed at startup).
    detected: Option<NodeResources>,
}

impl NodeAgent {
    pub fn new(api_client: Arc<ApiClient>, config: NodeAgentConfig) -> Self {
        let reservation = ResourceReservation {
            cpu_millicores: config.system_reserved_cpu_millicores,
            memory_bytes: config.system_reserved_memory_bytes,
        };

        let detected = match compute_node_resources(&reservation, config.max_pods) {
            Ok(nr) => {
                info!(
                    cpu_count = nr.capacity.cpu_count,
                    total_memory = %format_memory_quantity(nr.capacity.total_memory_bytes),
                    allocatable_cpu_m = nr.allocatable_cpu_millicores,
                    allocatable_memory = %format_memory_quantity(nr.allocatable_memory_bytes),
                    max_pods = nr.max_pods,
                    "Detected system resources"
                );
                Some(nr)
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to detect system resources, falling back to defaults"
                );
                None
            }
        };

        Self {
            api_client,
            config,
            detected,
        }
    }

    #[cfg(test)]
    fn new_with_detected(
        api_client: Arc<ApiClient>,
        config: NodeAgentConfig,
        detected: Option<NodeResources>,
    ) -> Self {
        Self {
            api_client,
            config,
            detected,
        }
    }

    /// Register this host as a Node resource
    pub async fn register(&self) -> Result<()> {
        info!("Registering node '{}'", self.config.node_name);

        let node = self.build_node();

        match self.api_client.create_node(&node).await {
            Ok(_) => {
                info!("Node '{}' registered successfully", self.config.node_name);
                Ok(())
            }
            Err(RuntimeError::ZoneAlreadyExists { .. }) => {
                // Node already exists — update its status instead
                info!(
                    "Node '{}' already exists, updating status",
                    self.config.node_name
                );
                self.api_client
                    .update_node_status(&self.config.node_name, &node)
                    .await?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Run the heartbeat loop
    pub async fn run(&self, token: CancellationToken) -> Result<()> {
        // Register first
        self.register().await?;

        info!(
            "Starting heartbeat loop (interval: {:?})",
            self.config.heartbeat_interval
        );

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("Node agent shutting down");
                    return Ok(());
                }
                _ = tokio::time::sleep(self.config.heartbeat_interval) => {
                    if let Err(e) = self.heartbeat().await {
                        warn!("Heartbeat failed: {} — will retry", e);
                    }
                }
            }
        }
    }

    /// Send a heartbeat by updating node status
    async fn heartbeat(&self) -> Result<()> {
        let node = self.build_node();

        self.api_client
            .update_node_status(&self.config.node_name, &node)
            .await?;

        info!("Heartbeat sent for node '{}'", self.config.node_name);
        Ok(())
    }

    /// Build the Node resource with current status
    fn build_node(&self) -> Node {
        let hostname = self.config.node_name.clone();

        let (capacity, allocatable) = if let Some(ref nr) = self.detected {
            let cap_cpu = nr.capacity.cpu_count.to_string();
            let cap_mem = format_memory_quantity(nr.capacity.total_memory_bytes);
            let pods = nr.max_pods.to_string();

            let alloc_cpu = format!("{}m", nr.allocatable_cpu_millicores);
            let alloc_mem = format_memory_quantity(nr.allocatable_memory_bytes);

            let capacity = BTreeMap::from([
                ("cpu".to_string(), Quantity(cap_cpu)),
                ("memory".to_string(), Quantity(cap_mem)),
                ("pods".to_string(), Quantity(pods.clone())),
            ]);

            let allocatable = BTreeMap::from([
                ("cpu".to_string(), Quantity(alloc_cpu)),
                ("memory".to_string(), Quantity(alloc_mem)),
                ("pods".to_string(), Quantity(pods)),
            ]);

            (capacity, allocatable)
        } else {
            // Fallback: use available_parallelism for CPU, hardcoded memory
            let cpu_count = std::thread::available_parallelism()
                .map(|n| n.get().to_string())
                .unwrap_or_else(|_| "1".to_string());

            let capacity = BTreeMap::from([
                ("cpu".to_string(), Quantity(cpu_count.clone())),
                ("memory".to_string(), Quantity("8Gi".to_string())),
                ("pods".to_string(), Quantity("110".to_string())),
            ]);

            let allocatable = BTreeMap::from([
                ("cpu".to_string(), Quantity(cpu_count)),
                ("memory".to_string(), Quantity("8Gi".to_string())),
                ("pods".to_string(), Quantity("110".to_string())),
            ]);

            (capacity, allocatable)
        };

        Node {
            metadata: ObjectMeta {
                name: Some(self.config.node_name.clone()),
                labels: Some(
                    [
                        ("kubernetes.io/hostname".to_string(), hostname.clone()),
                        (
                            "node.kubernetes.io/instance-type".to_string(),
                            "reddwarf-zone".to_string(),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            status: Some(NodeStatus {
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    reason: Some("KubeletReady".to_string()),
                    message: Some("reddwarf node agent is healthy".to_string()),
                    last_heartbeat_time: Some(
                        k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(chrono::Utc::now()),
                    ),
                    last_transition_time: Some(
                        k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(chrono::Utc::now()),
                    ),
                }]),
                addresses: Some(vec![NodeAddress {
                    type_: "Hostname".to_string(),
                    address: hostname,
                }]),
                allocatable: Some(allocatable),
                capacity: Some(capacity),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::detect_system_resources;

    #[test]
    fn test_node_agent_config_defaults() {
        let config =
            NodeAgentConfig::new("test-node".to_string(), "http://127.0.0.1:6443".to_string());
        assert_eq!(config.node_name, "test-node");
        assert_eq!(config.heartbeat_interval, Duration::from_secs(10));
        assert_eq!(config.system_reserved_cpu_millicores, 100);
        assert_eq!(config.system_reserved_memory_bytes, 256 * 1024 * 1024);
        assert_eq!(config.max_pods, 110);
    }

    #[test]
    fn test_build_node_has_ready_condition() {
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config =
            NodeAgentConfig::new("test-node".to_string(), "http://127.0.0.1:6443".to_string());
        let agent = NodeAgent::new(api_client, config);

        let node = agent.build_node();

        assert_eq!(node.metadata.name, Some("test-node".to_string()));
        let status = node.status.unwrap();
        let conditions = status.conditions.unwrap();
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].type_, "Ready");
        assert_eq!(conditions[0].status, "True");
        assert!(conditions[0].last_heartbeat_time.is_some());
    }

    #[test]
    fn test_build_node_has_allocatable_resources() {
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config =
            NodeAgentConfig::new("test-node".to_string(), "http://127.0.0.1:6443".to_string());
        let agent = NodeAgent::new(api_client, config);

        let node = agent.build_node();
        let status = node.status.unwrap();

        // Check allocatable has all keys
        let alloc = status.allocatable.unwrap();
        assert!(alloc.contains_key("cpu"));
        assert!(alloc.contains_key("memory"));
        assert!(alloc.contains_key("pods"));
        assert_eq!(alloc["pods"].0, "110");

        // Memory should come from detected system (not hardcoded 8Gi)
        let sys = detect_system_resources().expect("detection works in test");
        let expected_cap_mem = format_memory_quantity(sys.total_memory_bytes);
        let cap = status.capacity.unwrap();
        assert_eq!(cap["memory"].0, expected_cap_mem);

        // Capacity CPU should match detected cpu_count
        assert_eq!(cap["cpu"].0, sys.cpu_count.to_string());
    }

    #[test]
    fn test_build_node_allocatable_less_than_capacity() {
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config =
            NodeAgentConfig::new("test-node".to_string(), "http://127.0.0.1:6443".to_string());
        let agent = NodeAgent::new(api_client, config);

        // Agent should have detected resources (we're on a real host)
        assert!(agent.detected.is_some(), "detection should succeed in tests");

        let node = agent.build_node();
        let status = node.status.unwrap();
        let cap = status.capacity.unwrap();
        let alloc = status.allocatable.unwrap();

        // Allocatable CPU (millicores) should be less than capacity CPU (whole cores)
        let cap_cpu_m = reddwarf_core::resources::ResourceQuantities::parse_cpu(&cap["cpu"].0)
            .expect("valid cpu");
        let alloc_cpu_m =
            reddwarf_core::resources::ResourceQuantities::parse_cpu(&alloc["cpu"].0)
                .expect("valid cpu");
        assert!(
            alloc_cpu_m < cap_cpu_m,
            "allocatable CPU {}m should be less than capacity {}m",
            alloc_cpu_m,
            cap_cpu_m,
        );

        // Allocatable memory should be less than capacity memory
        let cap_mem =
            reddwarf_core::resources::ResourceQuantities::parse_memory(&cap["memory"].0)
                .expect("valid mem");
        let alloc_mem =
            reddwarf_core::resources::ResourceQuantities::parse_memory(&alloc["memory"].0)
                .expect("valid mem");
        assert!(
            alloc_mem < cap_mem,
            "allocatable memory {} should be less than capacity {}",
            alloc_mem,
            cap_mem,
        );
    }

    #[test]
    fn test_build_node_fallback_on_detection_failure() {
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config =
            NodeAgentConfig::new("test-node".to_string(), "http://127.0.0.1:6443".to_string());
        // Simulate detection failure
        let agent = NodeAgent::new_with_detected(api_client, config, None);

        let node = agent.build_node();
        let status = node.status.unwrap();
        let alloc = status.allocatable.unwrap();
        let cap = status.capacity.unwrap();

        // Should fall back to hardcoded defaults
        assert_eq!(alloc["memory"].0, "8Gi");
        assert_eq!(alloc["pods"].0, "110");
        assert_eq!(cap["memory"].0, "8Gi");
        assert_eq!(cap["pods"].0, "110");

        // CPU falls back to available_parallelism
        let expected_cpu = std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_else(|_| "1".to_string());
        assert_eq!(alloc["cpu"].0, expected_cpu);
        assert_eq!(cap["cpu"].0, expected_cpu);
    }
}
