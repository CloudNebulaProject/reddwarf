use crate::types::{FilterResult, ResourceQuantities, SchedulingContext};
use reddwarf_core::Node;
use tracing::debug;

/// Filter predicate trait
pub trait FilterPredicate: Send + Sync {
    /// Filter a node for the given pod
    fn filter(&self, context: &SchedulingContext, node: &Node) -> FilterResult;

    /// Name of the filter
    fn name(&self) -> &str;
}

/// Filter for pod resource requirements
pub struct PodFitsResources;

impl FilterPredicate for PodFitsResources {
    fn filter(&self, context: &SchedulingContext, node: &Node) -> FilterResult {
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

        // Get pod requested resources
        let pod_spec = match &context.pod.spec {
            Some(spec) => spec,
            None => return FilterResult::fail(node_name, "Pod has no spec".to_string()),
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

        debug!(
            "Node {} has CPU: {} milli, Memory: {} bytes",
            node_name, node_resources.cpu_millicores, node_resources.memory_bytes
        );
        debug!(
            "Pod requests CPU: {} milli, Memory: {} bytes",
            total_cpu, total_memory
        );

        // Check if node has enough resources
        if total_cpu > node_resources.cpu_millicores {
            return FilterResult::fail(
                node_name,
                format!(
                    "Insufficient CPU: requested {} milli, available {} milli",
                    total_cpu, node_resources.cpu_millicores
                ),
            );
        }

        if total_memory > node_resources.memory_bytes {
            return FilterResult::fail(
                node_name,
                format!(
                    "Insufficient memory: requested {} bytes, available {} bytes",
                    total_memory, node_resources.memory_bytes
                ),
            );
        }

        FilterResult::pass(node_name)
    }

    fn name(&self) -> &str {
        "PodFitsResources"
    }
}

/// Filter for node selector
pub struct NodeSelectorMatch;

impl FilterPredicate for NodeSelectorMatch {
    fn filter(&self, context: &SchedulingContext, node: &Node) -> FilterResult {
        let node_name = node
            .metadata
            .name
            .as_ref()
            .unwrap_or(&"unknown".to_string())
            .clone();

        let pod_spec = match &context.pod.spec {
            Some(spec) => spec,
            None => return FilterResult::pass(node_name),
        };

        // Get node selector from pod
        let node_selector = match &pod_spec.node_selector {
            Some(selector) => selector,
            None => return FilterResult::pass(node_name), // No selector = pass
        };

        // Get node labels
        let node_labels = node.metadata.labels.as_ref();

        // Check if all selector labels match
        for (key, value) in node_selector {
            let node_value = node_labels.and_then(|labels| labels.get(key));

            if node_value != Some(value) {
                return FilterResult::fail(
                    node_name,
                    format!("Node selector mismatch: {}={}", key, value),
                );
            }
        }

        FilterResult::pass(node_name)
    }

    fn name(&self) -> &str {
        "NodeSelectorMatch"
    }
}

/// Filter for taints and tolerations
pub struct TaintToleration;

impl FilterPredicate for TaintToleration {
    fn filter(&self, context: &SchedulingContext, node: &Node) -> FilterResult {
        let node_name = node
            .metadata
            .name
            .as_ref()
            .unwrap_or(&"unknown".to_string())
            .clone();

        // Get node taints
        let taints = match node.spec.as_ref().and_then(|s| s.taints.as_ref()) {
            Some(t) => t,
            None => return FilterResult::pass(node_name), // No taints = pass
        };

        // Get pod tolerations
        let pod_spec = match &context.pod.spec {
            Some(spec) => spec,
            None => return FilterResult::pass(node_name),
        };

        let tolerations = match &pod_spec.tolerations {
            Some(t) => t,
            None => {
                // No tolerations but node has taints = fail
                if !taints.is_empty() {
                    return FilterResult::fail(
                        node_name,
                        "Node has taints but pod has no tolerations".to_string(),
                    );
                }
                return FilterResult::pass(node_name);
            }
        };

        // Check if pod tolerates all taints
        for taint in taints {
            let taint_key = &taint.key;
            let taint_effect = &taint.effect;

            let mut tolerated = false;

            for toleration in tolerations {
                // Check if toleration matches taint
                let toleration_key = toleration.key.as_ref();
                let toleration_effect = toleration.effect.as_ref();

                if toleration_key.as_ref() == Some(&taint_key)
                    && (toleration_effect.is_none() || toleration_effect.as_ref() == Some(&taint_effect))
                {
                    tolerated = true;
                    break;
                }
            }

            if !tolerated {
                return FilterResult::fail(
                    node_name,
                    format!("Pod does not tolerate taint: {}={}", taint_key, taint_effect),
                );
            }
        }

        FilterResult::pass(node_name)
    }

    fn name(&self) -> &str {
        "TaintToleration"
    }
}

/// Get default filter predicates
pub fn default_filters() -> Vec<Box<dyn FilterPredicate>> {
    vec![
        Box::new(PodFitsResources),
        Box::new(NodeSelectorMatch),
        Box::new(TaintToleration),
    ]
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
    fn test_pod_fits_resources_pass() {
        let node = create_test_node("node1", "4", "8Gi");
        let pod = create_test_pod("1", "1Gi");
        let context = SchedulingContext::new(pod, vec![node.clone()]);

        let filter = PodFitsResources;
        let result = filter.filter(&context, &node);

        assert!(result.passed);
    }

    #[test]
    fn test_pod_fits_resources_fail_cpu() {
        let node = create_test_node("node1", "1", "8Gi");
        let pod = create_test_pod("2", "1Gi");
        let context = SchedulingContext::new(pod, vec![node.clone()]);

        let filter = PodFitsResources;
        let result = filter.filter(&context, &node);

        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("Insufficient CPU"));
    }

    #[test]
    fn test_pod_fits_resources_fail_memory() {
        let node = create_test_node("node1", "4", "1Gi");
        let pod = create_test_pod("1", "2Gi");
        let context = SchedulingContext::new(pod, vec![node.clone()]);

        let filter = PodFitsResources;
        let result = filter.filter(&context, &node);

        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("Insufficient memory"));
    }
}
