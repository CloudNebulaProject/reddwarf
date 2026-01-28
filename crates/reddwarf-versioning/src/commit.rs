use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Type of change in a commit
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    /// Resource created
    Create,
    /// Resource updated
    Update,
    /// Resource deleted
    Delete,
}

/// A change to a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    /// Type of change
    pub change_type: ChangeType,
    /// Resource key
    pub resource_key: String,
    /// Resource content (JSON-encoded)
    pub content: String,
    /// Previous content (for updates/deletes)
    pub previous_content: Option<String>,
}

impl Change {
    /// Create a new Change
    pub fn new(
        change_type: ChangeType,
        resource_key: String,
        content: String,
        previous_content: Option<String>,
    ) -> Self {
        Self {
            change_type,
            resource_key,
            content,
            previous_content,
        }
    }

    /// Create a Change for resource creation
    pub fn create(resource_key: String, content: String) -> Self {
        Self {
            change_type: ChangeType::Create,
            resource_key,
            content,
            previous_content: None,
        }
    }

    /// Create a Change for resource update
    pub fn update(resource_key: String, content: String, previous_content: String) -> Self {
        Self {
            change_type: ChangeType::Update,
            resource_key,
            content,
            previous_content: Some(previous_content),
        }
    }

    /// Create a Change for resource deletion
    pub fn delete(resource_key: String, previous_content: String) -> Self {
        Self {
            change_type: ChangeType::Delete,
            resource_key,
            content: String::new(),
            previous_content: Some(previous_content),
        }
    }
}

/// A commit in the version DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// Unique commit ID (UUID)
    pub id: String,
    /// Parent commit IDs (can have multiple for merges)
    pub parents: Vec<String>,
    /// Changes in this commit
    pub changes: Vec<Change>,
    /// Commit message
    pub message: String,
    /// Author
    pub author: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl Commit {
    /// Create a new commit
    pub fn new(
        parents: Vec<String>,
        changes: Vec<Change>,
        message: String,
        author: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            parents,
            changes,
            message,
            author,
            timestamp: Utc::now(),
        }
    }

    /// Get the commit ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Check if this is a merge commit
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Check if this is the root commit
    pub fn is_root(&self) -> bool {
        self.parents.is_empty()
    }
}

/// Builder for creating commits
pub struct CommitBuilder {
    parents: Vec<String>,
    changes: Vec<Change>,
    message: String,
    author: String,
}

impl CommitBuilder {
    /// Create a new CommitBuilder
    pub fn new() -> Self {
        Self {
            parents: Vec::new(),
            changes: Vec::new(),
            message: String::new(),
            author: "reddwarf".to_string(),
        }
    }

    /// Add a parent commit
    pub fn parent(mut self, parent_id: String) -> Self {
        self.parents.push(parent_id);
        self
    }

    /// Add multiple parent commits
    pub fn parents(mut self, parents: Vec<String>) -> Self {
        self.parents.extend(parents);
        self
    }

    /// Add a change
    pub fn change(mut self, change: Change) -> Self {
        self.changes.push(change);
        self
    }

    /// Add multiple changes
    pub fn changes(mut self, changes: Vec<Change>) -> Self {
        self.changes.extend(changes);
        self
    }

    /// Set the commit message
    pub fn message(mut self, message: String) -> Self {
        self.message = message;
        self
    }

    /// Set the author
    pub fn author(mut self, author: String) -> Self {
        self.author = author;
        self
    }

    /// Build the commit
    pub fn build(self) -> Commit {
        Commit::new(self.parents, self.changes, self.message, self.author)
    }
}

impl Default for CommitBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_create() {
        let change = Change::create("v1/Pod/default/nginx".to_string(), "{}".to_string());
        assert_eq!(change.change_type, ChangeType::Create);
        assert_eq!(change.resource_key, "v1/Pod/default/nginx");
        assert_eq!(change.previous_content, None);
    }

    #[test]
    fn test_change_update() {
        let change = Change::update(
            "v1/Pod/default/nginx".to_string(),
            "{\"new\":true}".to_string(),
            "{\"old\":true}".to_string(),
        );
        assert_eq!(change.change_type, ChangeType::Update);
        assert!(change.previous_content.is_some());
    }

    #[test]
    fn test_commit_creation() {
        let change = Change::create("v1/Pod/default/nginx".to_string(), "{}".to_string());
        let commit = CommitBuilder::new()
            .change(change)
            .message("Create nginx pod".to_string())
            .build();

        assert!(!commit.id.is_empty());
        assert_eq!(commit.changes.len(), 1);
        assert_eq!(commit.message, "Create nginx pod");
        assert!(commit.is_root());
        assert!(!commit.is_merge());
    }

    #[test]
    fn test_commit_with_parents() {
        let commit = CommitBuilder::new()
            .parent("parent1".to_string())
            .parent("parent2".to_string())
            .message("Merge commit".to_string())
            .build();

        assert_eq!(commit.parents.len(), 2);
        assert!(commit.is_merge());
        assert!(!commit.is_root());
    }
}
