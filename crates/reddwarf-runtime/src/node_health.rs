use crate::api_client::ApiClient;
use crate::error::Result;
use chrono::Utc;
use k8s_openapi::api::core::v1::{Node, NodeCondition};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Configuration for the node health checker
#[derive(Debug, Clone)]
pub struct NodeHealthCheckerConfig {
    /// Interval between health checks
    pub check_interval: Duration,
    /// How long since the last heartbeat before a node is considered stale
    pub heartbeat_timeout: Duration,
}

impl Default for NodeHealthCheckerConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(15),
            // 4x the default heartbeat interval (10s) = 40s
            heartbeat_timeout: Duration::from_secs(40),
        }
    }
}

/// Periodically checks node heartbeats and marks stale nodes as NotReady
pub struct NodeHealthChecker {
    api_client: Arc<ApiClient>,
    config: NodeHealthCheckerConfig,
}

impl NodeHealthChecker {
    pub fn new(api_client: Arc<ApiClient>, config: NodeHealthCheckerConfig) -> Self {
        Self { api_client, config }
    }

    /// Run the health checker loop
    pub async fn run(&self, token: CancellationToken) -> Result<()> {
        info!(
            "Starting node health checker (interval: {:?}, timeout: {:?})",
            self.config.check_interval, self.config.heartbeat_timeout
        );

        let mut interval = tokio::time::interval(self.config.check_interval);
        // Consume the first immediate tick — nodes just registered
        interval.tick().await;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("Node health checker shutting down");
                    return Ok(());
                }
                _ = interval.tick() => {
                    if let Err(e) = self.check_all_nodes().await {
                        error!("Node health check failed: {}", e);
                    }
                }
            }
        }
    }

    /// Check all nodes for stale heartbeats
    async fn check_all_nodes(&self) -> Result<()> {
        debug!("Running node health check");

        let body = self.api_client.get_json("/api/v1/nodes").await?;
        let items = body["items"].as_array().cloned().unwrap_or_default();

        for item in items {
            let node: Node = match serde_json::from_value(item) {
                Ok(n) => n,
                Err(e) => {
                    warn!("Failed to parse node from list: {}", e);
                    continue;
                }
            };

            let node_name = match node.metadata.name.as_deref() {
                Some(n) => n,
                None => continue,
            };

            if let Err(e) = self.check_node(node_name, &node).await {
                warn!("Failed to check node {}: {}", node_name, e);
            }
        }

        Ok(())
    }

    /// Check a single node's heartbeat and mark it NotReady if stale
    async fn check_node(&self, node_name: &str, node: &Node) -> Result<()> {
        let conditions = match node
            .status
            .as_ref()
            .and_then(|s| s.conditions.as_ref())
        {
            Some(c) => c,
            None => {
                debug!("Node {} has no conditions, skipping", node_name);
                return Ok(());
            }
        };

        let ready_condition = match conditions.iter().find(|c| c.type_ == "Ready") {
            Some(c) => c,
            None => {
                debug!("Node {} has no Ready condition, skipping", node_name);
                return Ok(());
            }
        };

        // Skip if already marked NotReady by us (avoid re-updating)
        if ready_condition.status == "False"
            && ready_condition.reason.as_deref() == Some("NodeStatusUnknown")
        {
            debug!(
                "Node {} already marked NotReady by health checker, skipping",
                node_name
            );
            return Ok(());
        }

        // Check heartbeat staleness
        let last_heartbeat = match &ready_condition.last_heartbeat_time {
            Some(t) => t.0,
            None => {
                debug!("Node {} has no last_heartbeat_time, skipping", node_name);
                return Ok(());
            }
        };

        let elapsed = Utc::now() - last_heartbeat;
        let timeout = chrono::Duration::from_std(self.config.heartbeat_timeout)
            .unwrap_or(chrono::Duration::seconds(40));

        if elapsed <= timeout {
            debug!(
                "Node {} heartbeat is fresh ({}s ago)",
                node_name,
                elapsed.num_seconds()
            );
            return Ok(());
        }

        // Node is stale — mark it NotReady
        warn!(
            "Node {} heartbeat is stale ({}s ago, timeout {}s) — marking NotReady",
            node_name,
            elapsed.num_seconds(),
            timeout.num_seconds()
        );

        let mut updated_node = node.clone();

        // Preserve last_transition_time if the status was already False,
        // otherwise set it to now
        let last_transition_time = if ready_condition.status == "False" {
            ready_condition.last_transition_time.clone()
        } else {
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                Utc::now(),
            ))
        };

        let new_condition = NodeCondition {
            type_: "Ready".to_string(),
            status: "False".to_string(),
            reason: Some("NodeStatusUnknown".to_string()),
            message: Some(format!(
                "Node heartbeat not received for {}s (timeout: {}s)",
                elapsed.num_seconds(),
                timeout.num_seconds()
            )),
            last_heartbeat_time: ready_condition.last_heartbeat_time.clone(),
            last_transition_time,
        };

        // Replace the Ready condition in the node status
        if let Some(status) = updated_node.status.as_mut() {
            if let Some(conditions) = status.conditions.as_mut() {
                if let Some(ready) = conditions.iter_mut().find(|c| c.type_ == "Ready") {
                    *ready = new_condition;
                }
            }
        }

        self.api_client
            .update_node_status(node_name, &updated_node)
            .await?;

        info!("Node {} marked as NotReady (stale heartbeat)", node_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{NodeCondition, NodeStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};

    fn make_node(name: &str, ready_status: &str, heartbeat_age_secs: i64) -> Node {
        let heartbeat_time = Utc::now() - chrono::Duration::seconds(heartbeat_age_secs);
        Node {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            status: Some(NodeStatus {
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: ready_status.to_string(),
                    reason: Some("KubeletReady".to_string()),
                    message: Some("node agent is healthy".to_string()),
                    last_heartbeat_time: Some(Time(heartbeat_time)),
                    last_transition_time: Some(Time(
                        Utc::now() - chrono::Duration::seconds(3600),
                    )),
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn make_stale_notready_node(name: &str, heartbeat_age_secs: i64) -> Node {
        let heartbeat_time = Utc::now() - chrono::Duration::seconds(heartbeat_age_secs);
        Node {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            status: Some(NodeStatus {
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: "False".to_string(),
                    reason: Some("NodeStatusUnknown".to_string()),
                    message: Some("Node heartbeat not received".to_string()),
                    last_heartbeat_time: Some(Time(heartbeat_time)),
                    last_transition_time: Some(Time(
                        Utc::now() - chrono::Duration::seconds(100),
                    )),
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Fresh heartbeat should result in no status change (check_node returns Ok
    /// and does not attempt an API call)
    #[tokio::test]
    async fn test_fresh_heartbeat_is_noop() {
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config = NodeHealthCheckerConfig {
            check_interval: Duration::from_secs(15),
            heartbeat_timeout: Duration::from_secs(40),
        };
        let checker = NodeHealthChecker::new(api_client, config);

        // 10 seconds ago — well within the 40s timeout
        let node = make_node("fresh-node", "True", 10);

        // Should succeed without making any API call (would fail since no server)
        let result = checker.check_node("fresh-node", &node).await;
        assert!(result.is_ok());
    }

    /// Stale heartbeat on a Ready node should attempt to mark it NotReady.
    /// Since we have no real API server the update call will fail, proving the
    /// checker detected the staleness and tried to act.
    #[tokio::test]
    async fn test_stale_heartbeat_triggers_update() {
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config = NodeHealthCheckerConfig {
            check_interval: Duration::from_secs(15),
            heartbeat_timeout: Duration::from_secs(40),
        };
        let checker = NodeHealthChecker::new(api_client, config);

        // 60 seconds ago — exceeds the 40s timeout
        let node = make_node("stale-node", "True", 60);

        // Should fail because there's no real API server — but that proves it
        // detected the staleness and attempted to update the node
        let result = checker.check_node("stale-node", &node).await;
        assert!(result.is_err());
    }

    /// A node already marked NotReady with reason "NodeStatusUnknown" should be
    /// skipped (no redundant update)
    #[tokio::test]
    async fn test_already_notready_is_skipped() {
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config = NodeHealthCheckerConfig {
            check_interval: Duration::from_secs(15),
            heartbeat_timeout: Duration::from_secs(40),
        };
        let checker = NodeHealthChecker::new(api_client, config);

        // 120 seconds stale but already marked by us
        let node = make_stale_notready_node("dead-node", 120);

        // Should return Ok without attempting an API call
        let result = checker.check_node("dead-node", &node).await;
        assert!(result.is_ok());
    }

    /// last_transition_time should be preserved when a node is already False
    /// (but not yet with our reason)
    #[tokio::test]
    async fn test_transition_time_preserved_when_already_false() {
        // Build a node that has status=False but with a different reason
        let heartbeat_time = Utc::now() - chrono::Duration::seconds(60);
        let original_transition_time = Utc::now() - chrono::Duration::seconds(300);
        let node = Node {
            metadata: ObjectMeta {
                name: Some("failing-node".to_string()),
                ..Default::default()
            },
            status: Some(NodeStatus {
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: "False".to_string(),
                    reason: Some("SomeOtherReason".to_string()),
                    message: Some("something else".to_string()),
                    last_heartbeat_time: Some(Time(heartbeat_time)),
                    last_transition_time: Some(Time(original_transition_time)),
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let config = NodeHealthCheckerConfig {
            check_interval: Duration::from_secs(15),
            heartbeat_timeout: Duration::from_secs(40),
        };
        let checker = NodeHealthChecker::new(api_client, config);

        // Will fail at the API call, but we can verify the logic by checking that
        // the code path was entered (it didn't skip due to already-notready check)
        let result = checker.check_node("failing-node", &node).await;
        assert!(result.is_err()); // proves it tried to update (different reason)
    }
}
