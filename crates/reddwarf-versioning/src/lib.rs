//! Reddwarf Versioning - DAG-based resource versioning with jj-lib
//!
//! This crate provides:
//! - VersionStore wrapper around jj-lib
//! - Commit operations for resource changes
//! - Conflict detection and representation
//! - DAG traversal for WATCH operations

pub mod commit;
pub mod conflict;
pub mod error;
pub mod store;

// Re-export commonly used types
pub use commit::{Change, ChangeType, Commit, CommitBuilder};
pub use conflict::{Conflict, ConflictSide};
pub use error::{Result, VersioningError};
pub use store::VersionStore;
