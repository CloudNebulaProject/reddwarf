//! Reddwarf Versioning - DAG-based resource versioning with jj-lib
//!
//! This crate provides:
//! - VersionStore wrapper around jj-lib
//! - Commit operations for resource changes
//! - Conflict detection and representation
//! - DAG traversal for WATCH operations

pub mod error;
pub mod store;
pub mod commit;
pub mod conflict;

// Re-export commonly used types
pub use error::{VersioningError, Result};
pub use store::VersionStore;
pub use commit::{Commit, CommitBuilder, Change, ChangeType};
pub use conflict::{Conflict, ConflictSide};
