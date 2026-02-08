use crate::api_client::ApiClient;
use crate::error::{Result, RuntimeError};
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
}

impl NodeAgentConfig {
    pub fn new(node_name: String, api_url: String) -> Self {
        Self {
            node_name,
            api_url,
            heartbeat_interval: Duration::from_secs(10),
        }
    }
}

/// Node agent that registers this host as a Node and sends periodic heartbeats
pub struct NodeAgent {
    api_client: Arc<ApiClient>,
    config: NodeAgentConfig,
}

impl NodeAgent {
    pub fn new(api_client: Arc<ApiClient>, config: NodeAgentConfig) -> Self {
        Self { api_client, config }
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

        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_else(|_| "1".to_string());

        let allocatable = BTreeMap::from([
            ("cpu".to_string(), Quantity(cpu_count.clone())),
            ("memory".to_string(), Quantity("8Gi".to_string())),
            ("pods".to_string(), Quantity("110".to_string())),
        ]);

        let capacity = BTreeMap::from([
            ("cpu".to_string(), Quantity(cpu_count)),
            ("memory".to_string(), Quantity("8Gi".to_string())),
            ("pods".to_string(), Quantity("110".to_string())),
        ]);

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

    #[test]
    fn test_node_agent_config_defaults() {
        let config =
            NodeAgentConfig::new("test-node".to_string(), "http://127.0.0.1:6443".to_string());
        assert_eq!(config.node_name, "test-node");
        assert_eq!(config.heartbeat_interval, Duration::from_secs(10));
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

        // Check allocatable
        let alloc = status.allocatable.unwrap();
        assert!(alloc.contains_key("cpu"));
        assert!(alloc.contains_key("memory"));
        assert!(alloc.contains_key("pods"));
        assert_eq!(alloc["memory"].0, "8Gi");
        assert_eq!(alloc["pods"].0, "110");

        // CPU should match available parallelism
        let expected_cpu = std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_else(|_| "1".to_string());
        assert_eq!(alloc["cpu"].0, expected_cpu);

        // Check capacity
        let cap = status.capacity.unwrap();
        assert!(cap.contains_key("cpu"));
        assert!(cap.contains_key("memory"));
        assert!(cap.contains_key("pods"));
        assert_eq!(cap["cpu"].0, expected_cpu);
    }
}
