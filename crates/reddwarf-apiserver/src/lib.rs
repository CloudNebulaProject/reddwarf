//! Reddwarf API Server - Kubernetes-compatible REST API
//!
//! This crate provides:
//! - Axum-based HTTP server
//! - Kubernetes API endpoints
//! - Resource handlers (GET, POST, PUT, PATCH, DELETE)
//! - LIST with filtering and pagination
//! - WATCH mechanism for streaming updates

pub mod error;
pub mod server;
pub mod handlers;
pub mod state;
pub mod response;
pub mod validation;
pub mod watch;

// Re-export commonly used types
pub use error::{ApiError, Result};
pub use server::{ApiServer, Config};
pub use state::AppState;
