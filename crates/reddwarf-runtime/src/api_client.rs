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
        Self::with_ca_cert(base_url, None)
    }

    /// Create a client that optionally trusts an additional CA certificate.
    ///
    /// When connecting to a server with a self-signed certificate, pass the
    /// CA PEM bytes here so the client will accept it.
    pub fn with_ca_cert(base_url: &str, ca_pem: Option<&[u8]>) -> Self {
        let mut builder = Client::builder();

        if let Some(pem) = ca_pem {
            if let Ok(cert) = reqwest::Certificate::from_pem(pem) {
                builder = builder.add_root_certificate(cert);
            }
        }

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: builder.build().unwrap_or_else(|_| Client::new()),
        }
    }

    /// Generic GET that returns a JSON value.
    pub async fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.base_url, path);
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
                "GET {} failed with status {}: {}",
                path, status, body
            )));
        }

        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("Failed to parse response: {}", e)))
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

    /// POST /api/v1/namespaces/{namespace}/pods/{name}/finalize
    ///
    /// Called by the controller after zone cleanup is complete to remove the pod
    /// from API server storage.
    pub async fn finalize_pod(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/namespaces/{}/pods/{}/finalize",
            self.base_url, namespace, name
        );
        debug!("POST {}", url);

        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| RuntimeError::internal_error(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(RuntimeError::internal_error(format!(
                "POST finalize pod failed with status {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_builds_client() {
        let client = ApiClient::new("http://127.0.0.1:6443");
        assert_eq!(client.base_url(), "http://127.0.0.1:6443");
    }

    #[test]
    fn test_with_ca_cert_none() {
        let client = ApiClient::with_ca_cert("https://127.0.0.1:6443", None);
        assert_eq!(client.base_url(), "https://127.0.0.1:6443");
    }

    #[test]
    fn test_with_ca_cert_invalid_pem_falls_back() {
        // Invalid PEM should not panic — just builds a client without the cert
        let client = ApiClient::with_ca_cert("https://127.0.0.1:6443", Some(b"not-a-pem"));
        assert_eq!(client.base_url(), "https://127.0.0.1:6443");
    }
}
