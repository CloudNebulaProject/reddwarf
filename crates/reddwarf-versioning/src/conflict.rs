use serde::{Deserialize, Serialize};

/// Represents one side of a conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictSide {
    /// Commit ID for this side
    pub commit_id: String,
    /// Content from this side
    pub content: String,
}

/// Represents a conflict between concurrent modifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Resource key that has the conflict
    pub resource_key: String,
    /// Our side of the conflict
    pub our_side: ConflictSide,
    /// Their side of the conflict
    pub their_side: ConflictSide,
    /// Base commit (common ancestor)
    pub base_commit_id: Option<String>,
}

impl Conflict {
    /// Create a new Conflict
    pub fn new(
        resource_key: String,
        our_side: ConflictSide,
        their_side: ConflictSide,
        base_commit_id: Option<String>,
    ) -> Self {
        Self {
            resource_key,
            our_side,
            their_side,
            base_commit_id,
        }
    }

    /// Get a description of the conflict
    pub fn description(&self) -> String {
        format!(
            "Conflict on resource {} between commits {} and {}",
            self.resource_key, self.our_side.commit_id, self.their_side.commit_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_creation() {
        let our_side = ConflictSide {
            commit_id: "commit1".to_string(),
            content: "{\"version\":1}".to_string(),
        };
        let their_side = ConflictSide {
            commit_id: "commit2".to_string(),
            content: "{\"version\":2}".to_string(),
        };

        let conflict = Conflict::new(
            "v1/Pod/default/nginx".to_string(),
            our_side,
            their_side,
            Some("base".to_string()),
        );

        assert_eq!(conflict.resource_key, "v1/Pod/default/nginx");
        assert_eq!(conflict.our_side.commit_id, "commit1");
        assert_eq!(conflict.their_side.commit_id, "commit2");
        assert!(conflict.description().contains("Conflict"));
    }
}
