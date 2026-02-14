pub use reddwarf_core::ResourceQuantities;
use reddwarf_core::{Node, Pod};

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

#[cfg(test)]
mod tests {
    use super::*;

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
