//! Reddwarf Scheduler - Pod to Node scheduling
//!
//! This crate provides:
//! - Pod scheduling algorithm
//! - Filter predicates (resource requirements, node selectors)
//! - Scoring functions (least allocated)
//! - Pod binding to nodes

pub mod error;
pub mod types;
pub mod filter;
pub mod score;
pub mod scheduler;

// Re-export commonly used types
pub use error::{SchedulerError, Result};
pub use scheduler::Scheduler;
pub use types::{SchedulingContext, FilterResult, ScoreResult};
