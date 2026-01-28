//! Reddwarf Scheduler - Pod to Node scheduling
//!
//! This crate provides:
//! - Pod scheduling algorithm
//! - Filter predicates (resource requirements, node selectors)
//! - Scoring functions (least allocated)
//! - Pod binding to nodes

pub mod error;
pub mod filter;
pub mod scheduler;
pub mod score;
pub mod types;

// Re-export commonly used types
pub use error::{Result, SchedulerError};
pub use scheduler::Scheduler;
pub use types::{FilterResult, SchedulingContext, ScoreResult};
