use crate::{Change, Commit, CommitBuilder, Conflict, ConflictSide, Result, VersioningError};
use reddwarf_storage::{KVStore, RedbBackend};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info};

/// Version store for managing DAG-based resource versions
pub struct VersionStore {
    storage: Arc<RedbBackend>,
    /// Current HEAD commit ID (latest commit)
    head: parking_lot::RwLock<Option<String>>,
}

impl VersionStore {
    /// Create a new VersionStore
    pub fn new(storage: Arc<RedbBackend>) -> Result<Self> {
        info!("Initializing VersionStore");

        let store = Self {
            storage,
            head: parking_lot::RwLock::new(None),
        };

        // Load HEAD from storage
        if let Some(head_bytes) = store.storage.get(b"version:head")? {
            let head_id = String::from_utf8_lossy(&head_bytes).to_string();
            info!("Loaded HEAD: {}", head_id);
            *store.head.write() = Some(head_id);
        }

        Ok(store)
    }

    /// Create a new commit
    pub fn create_commit(&self, builder: CommitBuilder) -> Result<Commit> {
        let commit = builder.build();
        debug!("Creating commit: {}", commit.id);

        // Serialize and store the commit
        let commit_json = serde_json::to_string(&commit)
            .map_err(|e| VersioningError::internal_error(format!("Failed to serialize commit: {}", e)))?;

        let commit_key = format!("version:commit:{}", commit.id);
        self.storage.put(commit_key.as_bytes(), commit_json.as_bytes())?;

        // Update HEAD
        self.set_head(commit.id.clone())?;

        info!("Created commit: {}", commit.id);
        Ok(commit)
    }

    /// Get a commit by ID
    pub fn get_commit(&self, commit_id: &str) -> Result<Commit> {
        debug!("Getting commit: {}", commit_id);

        let commit_key = format!("version:commit:{}", commit_id);
        let commit_bytes = self
            .storage
            .get(commit_key.as_bytes())?
            .ok_or_else(|| VersioningError::commit_not_found(commit_id))?;

        let commit: Commit = serde_json::from_slice(&commit_bytes)
            .map_err(|e| VersioningError::internal_error(format!("Failed to deserialize commit: {}", e)))?;

        Ok(commit)
    }

    /// Get the current HEAD commit
    pub fn get_head(&self) -> Result<Option<Commit>> {
        let head_id = self.head.read().clone();

        match head_id {
            Some(id) => Ok(Some(self.get_commit(&id)?)),
            None => Ok(None),
        }
    }

    /// Set the HEAD commit
    fn set_head(&self, commit_id: String) -> Result<()> {
        self.storage.put(b"version:head", commit_id.as_bytes())?;
        *self.head.write() = Some(commit_id);
        Ok(())
    }

    /// Get all commits (for debugging)
    pub fn list_commits(&self) -> Result<Vec<Commit>> {
        let keys = self.storage.keys_with_prefix(b"version:commit:")?;
        let mut commits = Vec::new();

        for key in keys {
            let commit_bytes = self.storage.get(&key)?.unwrap();
            let commit: Commit = serde_json::from_slice(&commit_bytes)
                .map_err(|e| VersioningError::internal_error(format!("Failed to deserialize commit: {}", e)))?;
            commits.push(commit);
        }

        Ok(commits)
    }

    /// Detect conflicts between two commits
    pub fn detect_conflicts(&self, commit_id1: &str, commit_id2: &str) -> Result<Vec<Conflict>> {
        debug!("Detecting conflicts between {} and {}", commit_id1, commit_id2);

        let commit1 = self.get_commit(commit_id1)?;
        let commit2 = self.get_commit(commit_id2)?;

        let mut conflicts = Vec::new();

        // Build maps of resource keys to changes
        let mut changes1: HashMap<String, &Change> = HashMap::new();
        for change in &commit1.changes {
            changes1.insert(change.resource_key.clone(), change);
        }

        let mut changes2: HashMap<String, &Change> = HashMap::new();
        for change in &commit2.changes {
            changes2.insert(change.resource_key.clone(), change);
        }

        // Find common resources that were modified in both commits
        for (resource_key, change1) in &changes1 {
            if let Some(change2) = changes2.get(resource_key) {
                // Both commits modified the same resource - potential conflict
                if change1.content != change2.content {
                    let conflict = Conflict::new(
                        resource_key.clone(),
                        ConflictSide {
                            commit_id: commit_id1.to_string(),
                            content: change1.content.clone(),
                        },
                        ConflictSide {
                            commit_id: commit_id2.to_string(),
                            content: change2.content.clone(),
                        },
                        self.find_common_ancestor(commit_id1, commit_id2)?,
                    );
                    conflicts.push(conflict);
                }
            }
        }

        if !conflicts.is_empty() {
            debug!("Found {} conflicts", conflicts.len());
        }

        Ok(conflicts)
    }

    /// Find the common ancestor of two commits (simplified BFS)
    pub fn find_common_ancestor(&self, commit_id1: &str, commit_id2: &str) -> Result<Option<String>> {
        let _commit1 = self.get_commit(commit_id1)?;
        let _commit2 = self.get_commit(commit_id2)?;

        // Get all ancestors of commit1
        let mut ancestors1 = HashSet::new();
        let mut to_visit = vec![commit_id1.to_string()];

        while let Some(commit_id) = to_visit.pop() {
            if ancestors1.contains(&commit_id) {
                continue;
            }
            ancestors1.insert(commit_id.clone());

            if let Ok(commit) = self.get_commit(&commit_id) {
                to_visit.extend(commit.parents);
            }
        }

        // Find first common ancestor in commit2's history
        let mut to_visit = vec![commit_id2.to_string()];
        let mut visited = HashSet::new();

        while let Some(commit_id) = to_visit.pop() {
            if visited.contains(&commit_id) {
                continue;
            }
            visited.insert(commit_id.clone());

            if ancestors1.contains(&commit_id) {
                return Ok(Some(commit_id));
            }

            if let Ok(commit) = self.get_commit(&commit_id) {
                to_visit.extend(commit.parents);
            }
        }

        Ok(None)
    }

    /// Traverse the DAG from one commit to another
    pub fn traverse(&self, from_commit_id: &str, to_commit_id: &str) -> Result<Vec<Commit>> {
        debug!("Traversing from {} to {}", from_commit_id, to_commit_id);

        let mut commits = Vec::new();
        let mut visited = HashSet::new();
        let mut to_visit = vec![to_commit_id.to_string()];

        // BFS from to_commit back to from_commit
        while let Some(commit_id) = to_visit.pop() {
            if commit_id == from_commit_id {
                break;
            }

            if visited.contains(&commit_id) {
                continue;
            }

            visited.insert(commit_id.clone());

            let commit = self.get_commit(&commit_id)?;
            commits.push(commit.clone());

            // Add parents to visit
            to_visit.extend(commit.parents);
        }

        // Reverse to get chronological order
        commits.reverse();

        Ok(commits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_storage::RedbBackend;
    use tempfile::tempdir;

    #[test]
    fn test_version_store_basic() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let backend = Arc::new(RedbBackend::new(&db_path).unwrap());
        let store = VersionStore::new(backend).unwrap();

        // Create a commit
        let change = Change::create("v1/Pod/default/nginx".to_string(), "{}".to_string());
        let commit = store
            .create_commit(CommitBuilder::new().change(change).message("Initial commit".to_string()))
            .unwrap();

        assert!(!commit.id.is_empty());

        // Get the commit back
        let retrieved = store.get_commit(&commit.id).unwrap();
        assert_eq!(retrieved.id, commit.id);
        assert_eq!(retrieved.message, "Initial commit");

        // Check HEAD
        let head = store.get_head().unwrap().unwrap();
        assert_eq!(head.id, commit.id);
    }

    #[test]
    fn test_conflict_detection() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let backend = Arc::new(RedbBackend::new(&db_path).unwrap());
        let store = VersionStore::new(backend).unwrap();

        // Create base commit
        let change1 = Change::create("v1/Pod/default/nginx".to_string(), "{\"version\":0}".to_string());
        let commit1 = store
            .create_commit(CommitBuilder::new().change(change1).message("Base".to_string()))
            .unwrap();

        // Create two diverging commits from the base
        let change2 = Change::update(
            "v1/Pod/default/nginx".to_string(),
            "{\"version\":1}".to_string(),
            "{\"version\":0}".to_string(),
        );
        let commit2 = store
            .create_commit(
                CommitBuilder::new()
                    .parent(commit1.id.clone())
                    .change(change2)
                    .message("Update A".to_string()),
            )
            .unwrap();

        let change3 = Change::update(
            "v1/Pod/default/nginx".to_string(),
            "{\"version\":2}".to_string(),
            "{\"version\":0}".to_string(),
        );
        let commit3 = store
            .create_commit(
                CommitBuilder::new()
                    .parent(commit1.id.clone())
                    .change(change3)
                    .message("Update B".to_string()),
            )
            .unwrap();

        // Detect conflicts
        let conflicts = store.detect_conflicts(&commit2.id, &commit3.id).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].resource_key, "v1/Pod/default/nginx");
    }
}
