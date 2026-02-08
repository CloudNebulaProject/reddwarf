use crate::error::{Result, RuntimeError};
use k8s_openapi::api::core::v1::{Node, Pod, PodStatus};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, warn};

/// Lightweight HTTP client for the controller/node-agent to talk to the API server
pub struct ApiClient {
    base_url: String,
    client: Client,
}

/// Watch event received from the API server SSE stream
#[derive(Debug, Clone, Deserialize)]
pub struct WatchEvent<T> {
    #[serde(rename = "type")]
    pub event_type: String,
    pub object: T,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    /// GET /api/v1/namespaces/{namespace}/pods/{name}
    pub async fn get_pod(&self, namespace: &str, name: &str) -> Result<Pod> {
        let url = format!(
            "{}/api/v1/namespaces/{}/pods/{}",
            self.base_url, namespace, name
        );
        debug!("GET {}", url);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RuntimeError::internal_error(format!(
                "GET pod failed with status {}: {}",
                status, body
            )));
        }

        resp.json::<Pod>()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("Failed to parse pod: {}", e)))
    }

    /// PUT /api/v1/namespaces/{namespace}/pods/{name}/status
    pub async fn update_pod_status(&self, namespace: &str, name: &str, pod: &Pod) -> Result<Pod> {
        let url = format!(
            "{}/api/v1/namespaces/{}/pods/{}/status",
            self.base_url, namespace, name
        );
        debug!("PUT {}", url);

        let resp = self
            .client
            .put(&url)
            .json(pod)
            .send()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RuntimeError::internal_error(format!(
                "PUT pod status failed with status {}: {}",
                status, body
            )));
        }

        resp.json::<Pod>()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("Failed to parse pod: {}", e)))
    }

    /// Build and update a Pod's status fields
    pub async fn set_pod_status(
        &self,
        namespace: &str,
        name: &str,
        status: PodStatus,
    ) -> Result<Pod> {
        // Get current pod to preserve metadata
        let mut pod = self.get_pod(namespace, name).await?;
        pod.status = Some(status);
        self.update_pod_status(namespace, name, &pod).await
    }

    /// POST /api/v1/nodes
    pub async fn create_node(&self, node: &Node) -> Result<Node> {
        let url = format!("{}/api/v1/nodes", self.base_url);
        debug!("POST {}", url);

        let resp = self
            .client
            .post(&url)
            .json(node)
            .send()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // 409 means node already exists — that's OK for re-registration
            if status == reqwest::StatusCode::CONFLICT {
                warn!("Node already exists, will update via status endpoint");
                return Err(RuntimeError::ZoneAlreadyExists {
                    zone_name: "node".to_string(),
                });
            }
            return Err(RuntimeError::internal_error(format!(
                "POST node failed with status {}: {}",
                status, body
            )));
        }

        resp.json::<Node>()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("Failed to parse node: {}", e)))
    }

    /// PUT /api/v1/nodes/{name}/status
    pub async fn update_node_status(&self, name: &str, node: &Node) -> Result<Node> {
        let url = format!("{}/api/v1/nodes/{}/status", self.base_url, name);
        debug!("PUT {}", url);

        let resp = self
            .client
            .put(&url)
            .json(node)
            .send()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RuntimeError::internal_error(format!(
                "PUT node status failed with status {}: {}",
                status, body
            )));
        }

        resp.json::<Node>()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("Failed to parse node: {}", e)))
    }

    /// GET /api/v1/nodes/{name}
    pub async fn get_node(&self, name: &str) -> Result<Node> {
        let url = format!("{}/api/v1/nodes/{}", self.base_url, name);
        debug!("GET {}", url);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RuntimeError::internal_error(format!(
                "GET node failed with status {}: {}",
                status, body
            )));
        }

        resp.json::<Node>()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("Failed to parse node: {}", e)))
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
