use crate::handlers::*;
use crate::AppState;
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::info;

/// API server configuration
#[derive(Clone)]
pub struct Config {
    /// Address to listen on
    pub listen_addr: SocketAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:6443".parse().unwrap(),
        }
    }
}

/// API server
pub struct ApiServer {
    config: Config,
    state: Arc<AppState>,
}

impl ApiServer {
    /// Create a new API server
    pub fn new(config: Config, state: Arc<AppState>) -> Self {
        Self { config, state }
    }

    /// Build the router
    fn build_router(&self) -> Router {
        Router::new()
            // Health checks
            .route("/healthz", get(healthz))
            .route("/livez", get(livez))
            .route("/readyz", get(readyz))
            // Pods
            .route(
                "/api/v1/namespaces/{namespace}/pods",
                get(list_pods).post(create_pod),
            )
            .route(
                "/api/v1/namespaces/{namespace}/pods/{name}",
                get(get_pod)
                    .put(replace_pod)
                    .patch(patch_pod)
                    .delete(delete_pod),
            )
            .route("/api/v1/pods", get(list_pods))
            // Nodes
            .route("/api/v1/nodes", get(list_nodes).post(create_node))
            .route(
                "/api/v1/nodes/{name}",
                get(get_node).put(replace_node).delete(delete_node),
            )
            // Services
            .route(
                "/api/v1/namespaces/{namespace}/services",
                get(list_services).post(create_service),
            )
            .route(
                "/api/v1/namespaces/{namespace}/services/{name}",
                get(get_service)
                    .put(replace_service)
                    .delete(delete_service),
            )
            // Namespaces
            .route(
                "/api/v1/namespaces",
                get(list_namespaces).post(create_namespace),
            )
            .route(
                "/api/v1/namespaces/{name}",
                get(get_namespace)
                    .put(replace_namespace)
                    .delete(delete_namespace),
            )
            // Add tracing and state
            .layer(TraceLayer::new_for_http())
            .with_state(self.state.clone())
    }

    /// Run the server
    pub async fn run(self) -> Result<(), std::io::Error> {
        let app = self.build_router();

        info!("Starting API server on {}", self.config.listen_addr);

        let listener = TcpListener::bind(self.config.listen_addr).await?;

        axum::serve(listener, app).await
    }
}

/// Health check endpoint
async fn healthz() -> &'static str {
    "ok"
}

/// Liveness probe
async fn livez() -> &'static str {
    "ok"
}

/// Readiness probe
async fn readyz() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_storage::RedbBackend;
    use reddwarf_versioning::VersionStore;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.listen_addr.to_string(), "127.0.0.1:6443");
    }

    #[test]
    fn test_build_router() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let storage = Arc::new(RedbBackend::new(&db_path).unwrap());
        let version_store = Arc::new(VersionStore::new(storage.clone()).unwrap());
        let state = Arc::new(AppState::new(storage, version_store));

        let server = ApiServer::new(Config::default(), state);
        let router = server.build_router();

        // Router should build successfully
        assert!(std::mem::size_of_val(&router) > 0);
    }
}
