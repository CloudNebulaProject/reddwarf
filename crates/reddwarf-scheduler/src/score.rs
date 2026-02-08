use crate::types::{ResourceQuantities, SchedulingContext, ScoreResult};
use reddwarf_core::Node;
use tracing::debug;

/// Scoring function trait
pub trait ScoreFunction: Send + Sync {
    /// Score a node for the given pod (0-100, higher is better)
    fn score(&self, context: &SchedulingContext, node: &Node) -> ScoreResult;

    /// Name of the scoring function
    fn name(&self) -> &str;
}

/// Score based on least allocated resources
pub struct LeastAllocated;

impl ScoreFunction for LeastAllocated {
    fn score(&self, context: &SchedulingContext, node: &Node) -> ScoreResult {
        let node_name = node
            .metadata
            .name
            .as_ref()
            .unwrap_or(&"unknown".to_string())
            .clone();

        // Get node allocatable resources
        let allocatable = node
            .status
            .as_ref()
            .and_then(|s| s.allocatable.as_ref())
            .cloned()
            .unwrap_or_default();

        let node_resources = ResourceQuantities::from_k8s_resource_map(&allocatable);

        // If node has no resources, score 0
        if node_resources.cpu_millicores == 0 || node_resources.memory_bytes == 0 {
            return ScoreResult::new(node_name, 0);
        }

        // Get pod requested resources
        let pod_spec = match &context.pod.spec {
            Some(spec) => spec,
            None => return ScoreResult::new(node_name, 0),
        };

        let mut total_cpu = 0i64;
        let mut total_memory = 0i64;

        for container in &pod_spec.containers {
            if let Some(resources) = &container.resources {
                if let Some(requests) = &resources.requests {
                    total_cpu += requests
                        .get("cpu")
                        .and_then(|s| ResourceQuantities::parse_cpu(&s.0).ok())
                        .unwrap_or(0);

                    total_memory += requests
                        .get("memory")
                        .and_then(|s| ResourceQuantities::parse_memory(&s.0).ok())
                        .unwrap_or(0);
                }
            }
        }

        // Calculate remaining resources after scheduling this pod
        let remaining_cpu = node_resources.cpu_millicores - total_cpu;
        let remaining_memory = node_resources.memory_bytes - total_memory;

        // Calculate utilization percentage for each resource
        let cpu_utilization = if node_resources.cpu_millicores > 0 {
            ((node_resources.cpu_millicores - remaining_cpu) as f64
                / node_resources.cpu_millicores as f64)
                * 100.0
        } else {
            100.0
        };

        let memory_utilization = if node_resources.memory_bytes > 0 {
            ((node_resources.memory_bytes - remaining_memory) as f64
                / node_resources.memory_bytes as f64)
                * 100.0
        } else {
            100.0
        };

        // Score is inverse of average utilization
        // Lower utilization = higher score (prefer less loaded nodes)
        let avg_utilization = (cpu_utilization + memory_utilization) / 2.0;
        let score = (100.0 - avg_utilization).clamp(0.0, 100.0) as i32;

        debug!(
            "Node {} score: {} (CPU util: {:.1}%, Memory util: {:.1}%)",
            node_name, score, cpu_utilization, memory_utilization
        );

        ScoreResult::new(node_name, score)
    }

    fn name(&self) -> &str {
        "LeastAllocated"
    }
}

/// Score based on balanced resource allocation
pub struct BalancedAllocation;

impl ScoreFunction for BalancedAllocation {
    fn score(&self, context: &SchedulingContext, node: &Node) -> ScoreResult {
        let node_name = node
            .metadata
            .name
            .as_ref()
            .unwrap_or(&"unknown".to_string())
            .clone();

        // Get node allocatable resources
        let allocatable = node
            .status
            .as_ref()
            .and_then(|s| s.allocatable.as_ref())
            .cloned()
            .unwrap_or_default();

        let node_resources = ResourceQuantities::from_k8s_resource_map(&allocatable);

        if node_resources.cpu_millicores == 0 || node_resources.memory_bytes == 0 {
            return ScoreResult::new(node_name, 0);
        }

        // Get pod requested resources
        let pod_spec = match &context.pod.spec {
            Some(spec) => spec,
            None => return ScoreResult::new(node_name, 50),
        };

        let mut total_cpu = 0i64;
        let mut total_memory = 0i64;

        for container in &pod_spec.containers {
            if let Some(resources) = &container.resources {
                if let Some(requests) = &resources.requests {
                    total_cpu += requests
                        .get("cpu")
                        .and_then(|s| ResourceQuantities::parse_cpu(&s.0).ok())
                        .unwrap_or(0);

                    total_memory += requests
                        .get("memory")
                        .and_then(|s| ResourceQuantities::parse_memory(&s.0).ok())
                        .unwrap_or(0);
                }
            }
        }

        // Calculate utilization after scheduling
        let cpu_fraction = total_cpu as f64 / node_resources.cpu_millicores as f64;
        let memory_fraction = total_memory as f64 / node_resources.memory_bytes as f64;

        // Prefer balanced resource usage (CPU and memory usage should be similar)
        let variance = (cpu_fraction - memory_fraction).abs();
        let score = ((1.0 - variance) * 100.0).clamp(0.0, 100.0) as i32;

        debug!(
            "Node {} balanced allocation score: {} (variance: {:.3})",
            node_name, score, variance
        );

        ScoreResult::new(node_name, score)
    }

    fn name(&self) -> &str {
        "BalancedAllocation"
    }
}

/// Get default scoring functions
pub fn default_scores() -> Vec<Box<dyn ScoreFunction>> {
    vec![Box::new(LeastAllocated), Box::new(BalancedAllocation)]
}

/// Calculate weighted score from multiple scoring functions
pub fn calculate_weighted_score(scores: &[ScoreResult]) -> i32 {
    if scores.is_empty() {
        return 0;
    }

    let total: i32 = scores.iter().map(|s| s.score).sum();
    total / scores.len() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_core::{Node, Pod};
    use std::collections::BTreeMap;

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

    fn create_test_pod(cpu: &str, memory: &str) -> Pod {
        let mut pod = Pod::default();
        pod.metadata.name = Some("test-pod".to_string());
        pod.spec = Some(Default::default());
        pod.spec.as_mut().unwrap().containers = vec![Default::default()];
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

    #[test]
    fn test_least_allocated() {
        let node1 = create_test_node("node1", "4", "8Gi");
        let node2 = create_test_node("node2", "4", "8Gi");
        let pod = create_test_pod("1", "2Gi");

        let context = SchedulingContext::new(pod, vec![node1.clone(), node2.clone()]);
        let scorer = LeastAllocated;

        let score1 = scorer.score(&context, &node1);
        let score2 = scorer.score(&context, &node2);

        // Both nodes should have same score (same resources, same request)
        assert_eq!(score1.score, score2.score);
        assert!(score1.score > 50); // Should prefer empty nodes
    }

    #[test]
    fn test_calculate_weighted_score() {
        let scores = vec![
            ScoreResult::new("node1".to_string(), 80),
            ScoreResult::new("node1".to_string(), 60),
        ];

        let weighted = calculate_weighted_score(&scores);
        assert_eq!(weighted, 70); // (80 + 60) / 2
    }
}
