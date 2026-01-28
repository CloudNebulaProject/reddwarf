use reddwarf_core::{Node, Pod};
use std::collections::HashMap;

/// Scheduling context containing pod and available nodes
#[derive(Debug, Clone)]
pub struct SchedulingContext {
    /// Pod to be scheduled
    pub pod: Pod,
    /// Available nodes
    pub nodes: Vec<Node>,
}

impl SchedulingContext {
    /// Create a new scheduling context
    pub fn new(pod: Pod, nodes: Vec<Node>) -> Self {
        Self { pod, nodes }
    }
}

/// Result of filtering a node
#[derive(Debug, Clone)]
pub struct FilterResult {
    /// Node name
    pub node_name: String,
    /// Whether the node passed the filter
    pub passed: bool,
    /// Reason for failure (if any)
    pub reason: Option<String>,
}

impl FilterResult {
    /// Create a passing filter result
    pub fn pass(node_name: String) -> Self {
        Self {
            node_name,
            passed: true,
            reason: None,
        }
    }

    /// Create a failing filter result
    pub fn fail(node_name: String, reason: String) -> Self {
        Self {
            node_name,
            passed: false,
            reason: Some(reason),
        }
    }
}

/// Result of scoring a node
#[derive(Debug, Clone)]
pub struct ScoreResult {
    /// Node name
    pub node_name: String,
    /// Score (0-100, higher is better)
    pub score: i32,
}

impl ScoreResult {
    /// Create a new score result
    pub fn new(node_name: String, score: i32) -> Self {
        Self { node_name, score }
    }
}

/// Resource quantities for nodes
#[derive(Debug, Clone, Default)]
pub struct ResourceQuantities {
    /// CPU in millicores (1000 = 1 core)
    pub cpu_millicores: i64,
    /// Memory in bytes
    pub memory_bytes: i64,
}

impl ResourceQuantities {
    /// Parse CPU string (e.g., "2", "1000m", "0.5")
    pub fn parse_cpu(s: &str) -> Result<i64, String> {
        if let Some(m) = s.strip_suffix('m') {
            // Millicores
            m.parse::<i64>()
                .map_err(|e| format!("Invalid CPU millicore value: {}", e))
        } else if let Ok(cores) = s.parse::<f64>() {
            // Cores as float
            Ok((cores * 1000.0) as i64)
        } else {
            Err(format!("Invalid CPU format: {}", s))
        }
    }

    /// Parse memory string (e.g., "128Mi", "1Gi", "1024")
    pub fn parse_memory(s: &str) -> Result<i64, String> {
        if let Some(num) = s.strip_suffix("Ki") {
            Ok(num.parse::<i64>().map_err(|e| e.to_string())? * 1024)
        } else if let Some(num) = s.strip_suffix("Mi") {
            Ok(num.parse::<i64>().map_err(|e| e.to_string())? * 1024 * 1024)
        } else if let Some(num) = s.strip_suffix("Gi") {
            Ok(num.parse::<i64>().map_err(|e| e.to_string())? * 1024 * 1024 * 1024)
        } else {
            // Plain bytes
            s.parse::<i64>().map_err(|e| e.to_string())
        }
    }

    /// Get CPU and memory from a resource map (k8s-openapi format)
    pub fn from_k8s_resource_map(
        resources: &std::collections::BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>,
    ) -> Self {
        let cpu_millicores = resources
            .get("cpu")
            .and_then(|q| Self::parse_cpu(&q.0).ok())
            .unwrap_or(0);

        let memory_bytes = resources
            .get("memory")
            .and_then(|q| Self::parse_memory(&q.0).ok())
            .unwrap_or(0);

        Self {
            cpu_millicores,
            memory_bytes,
        }
    }

    /// Get CPU and memory from a resource map (test format)
    pub fn from_resource_map(resources: &HashMap<String, String>) -> Self {
        let cpu_millicores = resources
            .get("cpu")
            .and_then(|s| Self::parse_cpu(s).ok())
            .unwrap_or(0);

        let memory_bytes = resources
            .get("memory")
            .and_then(|s| Self::parse_memory(s).ok())
            .unwrap_or(0);

        Self {
            cpu_millicores,
            memory_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu() {
        assert_eq!(ResourceQuantities::parse_cpu("1").unwrap(), 1000);
        assert_eq!(ResourceQuantities::parse_cpu("0.5").unwrap(), 500);
        assert_eq!(ResourceQuantities::parse_cpu("100m").unwrap(), 100);
        assert_eq!(ResourceQuantities::parse_cpu("2").unwrap(), 2000);
    }

    #[test]
    fn test_parse_memory() {
        assert_eq!(ResourceQuantities::parse_memory("1024").unwrap(), 1024);
        assert_eq!(ResourceQuantities::parse_memory("1Ki").unwrap(), 1024);
        assert_eq!(
            ResourceQuantities::parse_memory("128Mi").unwrap(),
            128 * 1024 * 1024
        );
        assert_eq!(
            ResourceQuantities::parse_memory("1Gi").unwrap(),
            1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_filter_result() {
        let pass = FilterResult::pass("node1".to_string());
        assert!(pass.passed);
        assert!(pass.reason.is_none());

        let fail = FilterResult::fail("node2".to_string(), "Insufficient CPU".to_string());
        assert!(!fail.passed);
        assert_eq!(fail.reason, Some("Insufficient CPU".to_string()));
    }
}
