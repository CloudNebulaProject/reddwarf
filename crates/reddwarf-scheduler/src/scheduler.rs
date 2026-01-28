use crate::filter::{default_filters, FilterPredicate};
use crate::score::{calculate_weighted_score, default_scores, ScoreFunction};
use crate::types::SchedulingContext;
use crate::{Result, SchedulerError};
use reddwarf_core::{Node, Pod};
use reddwarf_storage::{KVStore, KeyEncoder, RedbBackend};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Configuration for the scheduler
#[derive(Clone)]
pub struct SchedulerConfig {
    /// Interval between scheduling cycles
    pub schedule_interval: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            schedule_interval: Duration::from_secs(1),
        }
    }
}

/// Pod scheduler
pub struct Scheduler {
    storage: Arc<RedbBackend>,
    config: SchedulerConfig,
    filters: Vec<Box<dyn FilterPredicate>>,
    scorers: Vec<Box<dyn ScoreFunction>>,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new(storage: Arc<RedbBackend>, config: SchedulerConfig) -> Self {
        Self {
            storage,
            config,
            filters: default_filters(),
            scorers: default_scores(),
        }
    }

    /// Run the scheduler loop
    pub async fn run(&self) -> Result<()> {
        info!("Starting scheduler");

        loop {
            if let Err(e) = self.schedule_cycle().await {
                error!("Scheduling cycle failed: {}", e);
            }

            sleep(self.config.schedule_interval).await;
        }
    }

    /// Run a single scheduling cycle
    async fn schedule_cycle(&self) -> Result<()> {
        debug!("Running scheduling cycle");

        // Get all unscheduled pods
        let unscheduled_pods = self.get_unscheduled_pods().await?;

        if unscheduled_pods.is_empty() {
            debug!("No unscheduled pods found");
            return Ok(());
        }

        info!("Found {} unscheduled pods", unscheduled_pods.len());

        // Get all available nodes
        let nodes = self.get_nodes().await?;

        if nodes.is_empty() {
            warn!("No nodes available for scheduling");
            return Ok(());
        }

        info!("Found {} available nodes", nodes.len());

        // Schedule each pod
        for pod in unscheduled_pods {
            let pod_name = pod
                .metadata
                .name
                .as_ref()
                .unwrap_or(&"unknown".to_string())
                .clone();

            match self.schedule_pod(pod, &nodes).await {
                Ok(node_name) => {
                    info!("Scheduled pod {} to node {}", pod_name, node_name);
                }
                Err(e) => {
                    error!("Failed to schedule pod {}: {}", pod_name, e);
                }
            }
        }

        Ok(())
    }

    /// Get all unscheduled pods (spec.nodeName is empty)
    async fn get_unscheduled_pods(&self) -> Result<Vec<Pod>> {
        let prefix = KeyEncoder::encode_prefix("v1", "Pod", None);
        let results = self.storage.as_ref().scan(prefix.as_bytes())?;

        let mut unscheduled = Vec::new();

        for (_key, data) in results.iter() {
            let pod: Pod = serde_json::from_slice(data).map_err(|e| {
                SchedulerError::internal_error(format!("Failed to deserialize pod: {}", e))
            })?;

            // Check if pod is unscheduled
            if let Some(spec) = &pod.spec {
                if spec.node_name.is_none() {
                    unscheduled.push(pod);
                }
            }
        }

        Ok(unscheduled)
    }

    /// Get all nodes
    async fn get_nodes(&self) -> Result<Vec<Node>> {
        let prefix = KeyEncoder::encode_prefix("v1", "Node", None);
        let results = self.storage.as_ref().scan(prefix.as_bytes())?;

        let mut nodes = Vec::new();

        for (_key, data) in results.iter() {
            let node: Node = serde_json::from_slice(data).map_err(|e| {
                SchedulerError::internal_error(format!("Failed to deserialize node: {}", e))
            })?;
            nodes.push(node);
        }

        Ok(nodes)
    }

    /// Schedule a single pod
    async fn schedule_pod(&self, mut pod: Pod, nodes: &[Node]) -> Result<String> {
        let pod_name = pod
            .metadata
            .name
            .as_ref()
            .ok_or_else(|| SchedulerError::internal_error("Pod has no name"))?
            .clone();

        let context = SchedulingContext::new(pod.clone(), nodes.to_vec());

        // Phase 1: Filter nodes
        let mut feasible_nodes = Vec::new();

        for node in nodes {
            let node_name = node
                .metadata
                .name
                .as_ref()
                .unwrap_or(&"unknown".to_string())
                .clone();

            let mut passed = true;

            for filter in &self.filters {
                let result = filter.filter(&context, node);
                if !result.passed {
                    debug!(
                        "Node {} filtered out by {}: {}",
                        node_name,
                        filter.name(),
                        result.reason.unwrap_or_default()
                    );
                    passed = false;
                    break;
                }
            }

            if passed {
                feasible_nodes.push(node.clone());
            }
        }

        if feasible_nodes.is_empty() {
            return Err(SchedulerError::no_suitable_nodes(
                pod_name,
                "All nodes filtered out".to_string(),
            ));
        }

        info!(
            "Pod {} has {} feasible nodes",
            pod_name,
            feasible_nodes.len()
        );

        // Phase 2: Score nodes
        let mut node_scores: Vec<(String, i32)> = Vec::new();

        for node in &feasible_nodes {
            let node_name = node
                .metadata
                .name
                .as_ref()
                .unwrap_or(&"unknown".to_string())
                .clone();

            let mut scores = Vec::new();

            for scorer in &self.scorers {
                let score = scorer.score(&context, node);
                scores.push(score);
            }

            let final_score = calculate_weighted_score(&scores);
            node_scores.push((node_name, final_score));
        }

        // Phase 3: Select best node
        node_scores.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by score descending

        let best_node = node_scores
            .first()
            .ok_or_else(|| SchedulerError::internal_error("No nodes scored"))?
            .0
            .clone();

        info!(
            "Selected node {} for pod {} with score {}",
            best_node, pod_name, node_scores[0].1
        );

        // Phase 4: Bind pod to node
        self.bind_pod(&mut pod, &best_node).await?;

        Ok(best_node)
    }

    /// Bind a pod to a node (update spec.nodeName)
    async fn bind_pod(&self, pod: &mut Pod, node_name: &str) -> Result<()> {
        let pod_name = pod
            .metadata
            .name
            .as_ref()
            .ok_or_else(|| SchedulerError::internal_error("Pod has no name"))?;
        let namespace = pod
            .metadata
            .namespace
            .as_ref()
            .ok_or_else(|| SchedulerError::internal_error("Pod has no namespace"))?;

        info!("Binding pod {} to node {}", pod_name, node_name);

        // Update pod spec
        if let Some(spec) = &mut pod.spec {
            spec.node_name = Some(node_name.to_string());
        } else {
            return Err(SchedulerError::internal_error("Pod has no spec"));
        }

        // Save updated pod
        let key = reddwarf_core::ResourceKey::new(
            reddwarf_core::GroupVersionKind::from_api_version_kind("v1", "Pod"),
            namespace,
            pod_name,
        );

        let storage_key = KeyEncoder::encode_resource_key(&key);
        let data = serde_json::to_vec(&pod).map_err(|e| {
            SchedulerError::internal_error(format!("Failed to serialize pod: {}", e))
        })?;

        self.storage.as_ref().put(storage_key.as_bytes(), &data)?;

        info!("Successfully bound pod {} to node {}", pod_name, node_name);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_storage::RedbBackend;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn create_test_node(name: &str, cpu: &str, memory: &str) -> Node {
        let mut node = Node::default();
        node.metadata.name = Some(name.to_string());
        node.status = Some(Default::default());
        node.status.as_mut().unwrap().allocatable = Some(BTreeMap::new());
        node.status
            .as_mut()
            .unwrap()
            .allocatable
            .as_mut()
            .unwrap()
            .insert(
                "cpu".to_string(),
                k8s_openapi::apimachinery::pkg::api::resource::Quantity(cpu.to_string()),
            );
        node.status
            .as_mut()
            .unwrap()
            .allocatable
            .as_mut()
            .unwrap()
            .insert(
                "memory".to_string(),
                k8s_openapi::apimachinery::pkg::api::resource::Quantity(memory.to_string()),
            );
        node
    }

    fn create_test_pod(name: &str, namespace: &str, cpu: &str, memory: &str) -> Pod {
        let mut pod = Pod::default();
        pod.metadata.name = Some(name.to_string());
        pod.metadata.namespace = Some(namespace.to_string());
        pod.spec = Some(Default::default());
        pod.spec.as_mut().unwrap().containers = vec![Default::default()];
        pod.spec.as_mut().unwrap().containers[0].name = "test".to_string();
        pod.spec.as_mut().unwrap().containers[0].resources = Some(Default::default());
        pod.spec.as_mut().unwrap().containers[0]
            .resources
            .as_mut()
            .unwrap()
            .requests = Some(BTreeMap::new());
        pod.spec.as_mut().unwrap().containers[0]
            .resources
            .as_mut()
            .unwrap()
            .requests
            .as_mut()
            .unwrap()
            .insert(
                "cpu".to_string(),
                k8s_openapi::apimachinery::pkg::api::resource::Quantity(cpu.to_string()),
            );
        pod.spec.as_mut().unwrap().containers[0]
            .resources
            .as_mut()
            .unwrap()
            .requests
            .as_mut()
            .unwrap()
            .insert(
                "memory".to_string(),
                k8s_openapi::apimachinery::pkg::api::resource::Quantity(memory.to_string()),
            );
        pod
    }

    #[tokio::test]
    async fn test_schedule_pod_success() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let storage = Arc::new(RedbBackend::new(&db_path).unwrap());

        let scheduler = Scheduler::new(storage.clone(), SchedulerConfig::default());

        // Create nodes
        let nodes = vec![
            create_test_node("node1", "4", "8Gi"),
            create_test_node("node2", "2", "4Gi"),
        ];

        // Create pod
        let pod = create_test_pod("test-pod", "default", "1", "1Gi");

        // Schedule pod
        let result = scheduler.schedule_pod(pod, &nodes).await;

        assert!(result.is_ok());
        let node_name = result.unwrap();
        assert!(node_name == "node1" || node_name == "node2");
    }

    #[tokio::test]
    async fn test_schedule_pod_no_suitable_nodes() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let storage = Arc::new(RedbBackend::new(&db_path).unwrap());

        let scheduler = Scheduler::new(storage, SchedulerConfig::default());

        // Create node with insufficient resources
        let nodes = vec![create_test_node("node1", "1", "1Gi")];

        // Create pod that requires more resources
        let pod = create_test_pod("test-pod", "default", "2", "2Gi");

        // Schedule pod should fail
        let result = scheduler.schedule_pod(pod, &nodes).await;

        assert!(result.is_err());
    }
}
