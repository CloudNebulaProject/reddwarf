use crate::api_client::ApiClient;
use crate::error::{Result, RuntimeError};
use crate::traits::ZoneRuntime;
use crate::types::*;
use k8s_openapi::api::core::v1::{Pod, PodCondition, PodStatus};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Configuration for the pod controller
#[derive(Debug, Clone)]
pub struct PodControllerConfig {
    /// Only reconcile pods assigned to this node
    pub node_name: String,
    /// API server URL (e.g., "http://127.0.0.1:6443")
    pub api_url: String,
    /// Prefix for zone root paths (e.g., "/zones")
    pub zonepath_prefix: String,
    /// Parent ZFS dataset (e.g., "rpool/zones")
    pub zfs_parent_dataset: String,
    /// Default zone brand
    pub default_brand: ZoneBrand,
    /// Default network configuration
    pub network: NetworkMode,
}

/// Pod controller that watches for Pod events and drives zone lifecycle
pub struct PodController {
    runtime: Arc<dyn ZoneRuntime>,
    api_client: Arc<ApiClient>,
    config: PodControllerConfig,
}

impl PodController {
    pub fn new(
        runtime: Arc<dyn ZoneRuntime>,
        api_client: Arc<ApiClient>,
        config: PodControllerConfig,
    ) -> Self {
        Self {
            runtime,
            api_client,
            config,
        }
    }

    /// Run the controller — polls for unscheduled-to-this-node pods in a loop.
    ///
    /// In a real implementation, this would use SSE watch. For now, we receive
    /// events via the in-process event bus by subscribing to the broadcast channel.
    /// Since the controller runs in the same process as the API server, we use
    /// a simpler polling approach over the HTTP API.
    pub async fn run(&self) -> Result<()> {
        info!(
            "Starting pod controller for node '{}'",
            self.config.node_name
        );

        // Poll loop — watches for pods via HTTP list
        loop {
            if let Err(e) = self.reconcile_all().await {
                error!("Pod controller reconcile cycle failed: {}", e);
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    /// Reconcile all pods assigned to this node
    async fn reconcile_all(&self) -> Result<()> {
        debug!("Running pod controller reconcile cycle");

        // List all pods via the API
        let url = format!("{}/api/v1/pods", self.api_client.base_url());
        let resp = reqwest::get(&url)
            .await
            .map_err(|e| RuntimeError::internal_error(format!("Failed to list pods: {}", e)))?;

        if !resp.status().is_success() {
            return Err(RuntimeError::internal_error("Failed to list pods"));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            RuntimeError::internal_error(format!("Failed to parse pod list: {}", e))
        })?;

        let items = body["items"].as_array().cloned().unwrap_or_default();

        for item in items {
            let pod: Pod = match serde_json::from_value(item) {
                Ok(p) => p,
                Err(e) => {
                    warn!("Failed to parse pod from list: {}", e);
                    continue;
                }
            };

            if let Err(e) = self.reconcile(&pod).await {
                let pod_name = pod.metadata.name.as_deref().unwrap_or("<unknown>");
                error!("Failed to reconcile pod {}: {}", pod_name, e);
            }
        }

        Ok(())
    }

    /// Reconcile a single Pod event
    pub async fn reconcile(&self, pod: &Pod) -> Result<()> {
        let pod_name = pod
            .metadata
            .name
            .as_deref()
            .ok_or_else(|| RuntimeError::internal_error("Pod has no name"))?;
        let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");

        let spec = match &pod.spec {
            Some(s) => s,
            None => {
                debug!("Skipping pod {} — no spec", pod_name);
                return Ok(());
            }
        };

        // Only reconcile pods assigned to this node
        let node_name = match &spec.node_name {
            Some(n) => n.as_str(),
            None => {
                debug!("Skipping pod {} — not yet scheduled", pod_name);
                return Ok(());
            }
        };

        if node_name != self.config.node_name {
            return Ok(());
        }

        // Check current phase
        let phase = pod
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or("");

        let zone_name = pod_zone_name(namespace, pod_name);

        match phase {
            "" | "Pending" => {
                // Pod is assigned to us but has no phase — provision it
                info!("Provisioning zone for pod {}/{}", namespace, pod_name);
                let zone_config = pod_to_zone_config(pod, &self.config)?;

                match self.runtime.provision(&zone_config).await {
                    Ok(()) => {
                        info!("Zone {} provisioned successfully", zone_name);
                        // Update pod status to Running
                        let status = PodStatus {
                            phase: Some("Running".to_string()),
                            conditions: Some(vec![PodCondition {
                                type_: "Ready".to_string(),
                                status: "True".to_string(),
                                ..Default::default()
                            }]),
                            pod_ip: Some(self.zone_ip(&zone_config)),
                            ..Default::default()
                        };

                        if let Err(e) = self
                            .api_client
                            .set_pod_status(namespace, pod_name, status)
                            .await
                        {
                            error!("Failed to update pod status to Running: {}", e);
                        }
                    }
                    Err(e) => {
                        // Check if it's already provisioned (zone already exists)
                        if matches!(e, RuntimeError::ZoneAlreadyExists { .. }) {
                            debug!("Zone {} already exists, checking state", zone_name);
                            return Ok(());
                        }
                        error!("Failed to provision zone {}: {}", zone_name, e);
                        let status = PodStatus {
                            phase: Some("Failed".to_string()),
                            conditions: Some(vec![PodCondition {
                                type_: "Ready".to_string(),
                                status: "False".to_string(),
                                message: Some(format!("Zone provisioning failed: {}", e)),
                                ..Default::default()
                            }]),
                            ..Default::default()
                        };

                        if let Err(e2) = self
                            .api_client
                            .set_pod_status(namespace, pod_name, status)
                            .await
                        {
                            error!("Failed to update pod status to Failed: {}", e2);
                        }
                    }
                }
            }
            "Running" => {
                // Check zone health
                match self.runtime.get_zone_state(&zone_name).await {
                    Ok(ZoneState::Running) => {
                        // All good
                    }
                    Ok(state) => {
                        warn!(
                            "Zone {} is in unexpected state: {} (expected Running)",
                            zone_name, state
                        );
                        let status = PodStatus {
                            phase: Some("Failed".to_string()),
                            conditions: Some(vec![PodCondition {
                                type_: "Ready".to_string(),
                                status: "False".to_string(),
                                message: Some(format!("Zone is in unexpected state: {}", state)),
                                ..Default::default()
                            }]),
                            ..Default::default()
                        };

                        if let Err(e) = self
                            .api_client
                            .set_pod_status(namespace, pod_name, status)
                            .await
                        {
                            error!("Failed to update pod status to Failed: {}", e);
                        }
                    }
                    Err(RuntimeError::ZoneNotFound { .. }) => {
                        warn!(
                            "Zone {} not found but pod is Running — marking Failed",
                            zone_name
                        );
                        let status = PodStatus {
                            phase: Some("Failed".to_string()),
                            conditions: Some(vec![PodCondition {
                                type_: "Ready".to_string(),
                                status: "False".to_string(),
                                message: Some("Zone not found".to_string()),
                                ..Default::default()
                            }]),
                            ..Default::default()
                        };

                        if let Err(e) = self
                            .api_client
                            .set_pod_status(namespace, pod_name, status)
                            .await
                        {
                            error!("Failed to update pod status to Failed: {}", e);
                        }
                    }
                    Err(e) => {
                        debug!("Could not check zone state for {}: {}", zone_name, e);
                    }
                }
            }
            _ => {
                debug!(
                    "Pod {}/{} in phase {} — no action needed",
                    namespace, pod_name, phase
                );
            }
        }

        Ok(())
    }

    /// Handle pod deletion — deprovision the zone
    pub async fn handle_delete(&self, pod: &Pod) -> Result<()> {
        let pod_name = pod
            .metadata
            .name
            .as_deref()
            .ok_or_else(|| RuntimeError::internal_error("Pod has no name"))?;
        let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");

        // Only deprovision pods assigned to this node
        if let Some(spec) = &pod.spec {
            if let Some(node_name) = &spec.node_name {
                if node_name != &self.config.node_name {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        let zone_config = pod_to_zone_config(pod, &self.config)?;
        info!(
            "Deprovisioning zone for deleted pod {}/{}",
            namespace, pod_name
        );

        if let Err(e) = self.runtime.deprovision(&zone_config).await {
            warn!(
                "Failed to deprovision zone for pod {}/{}: {}",
                namespace, pod_name, e
            );
        }

        Ok(())
    }

    /// Extract IP address from zone config network
    fn zone_ip(&self, config: &ZoneConfig) -> String {
        match &config.network {
            NetworkMode::Etherstub(e) => e.ip_address.clone(),
            NetworkMode::Direct(d) => d.ip_address.clone(),
        }
    }
}

/// Generate a zone name from namespace and pod name
///
/// Zone names must be valid illumos zone names (alphanumeric, hyphens, max 64 chars).
pub fn pod_zone_name(namespace: &str, pod_name: &str) -> String {
    let raw = format!("reddwarf-{}-{}", namespace, pod_name);
    // Sanitize: only keep alphanumeric and hyphens, truncate to 64 chars
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.len() > 64 {
        sanitized[..64].to_string()
    } else {
        sanitized
    }
}

/// Convert a Pod spec to a ZoneConfig for the runtime
pub fn pod_to_zone_config(pod: &Pod, config: &PodControllerConfig) -> Result<ZoneConfig> {
    let pod_name = pod
        .metadata
        .name
        .as_deref()
        .ok_or_else(|| RuntimeError::internal_error("Pod has no name"))?;
    let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");

    let spec = pod
        .spec
        .as_ref()
        .ok_or_else(|| RuntimeError::internal_error("Pod has no spec"))?;

    let zone_name = pod_zone_name(namespace, pod_name);
    let zonepath = format!("{}/{}", config.zonepath_prefix, zone_name);

    // Map containers to ContainerProcess entries
    let processes: Vec<ContainerProcess> = spec
        .containers
        .iter()
        .map(|c| {
            let command = c
                .command
                .clone()
                .unwrap_or_default()
                .into_iter()
                .chain(c.args.clone().unwrap_or_default())
                .collect::<Vec<_>>();

            let env = c
                .env
                .as_ref()
                .map(|envs| {
                    envs.iter()
                        .filter_map(|e| e.value.as_ref().map(|v| (e.name.clone(), v.clone())))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            ContainerProcess {
                name: c.name.clone(),
                command,
                working_dir: c.working_dir.clone(),
                env,
            }
        })
        .collect();

    Ok(ZoneConfig {
        zone_name,
        brand: config.default_brand.clone(),
        zonepath,
        network: config.network.clone(),
        zfs: ZfsConfig {
            parent_dataset: config.zfs_parent_dataset.clone(),
            clone_from: None,
            quota: None,
        },
        lx_image_path: None,
        processes,
        cpu_cap: None,
        memory_cap: None,
        fs_mounts: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{Container, PodSpec};

    #[test]
    fn test_pod_zone_name_basic() {
        assert_eq!(pod_zone_name("default", "nginx"), "reddwarf-default-nginx");
    }

    #[test]
    fn test_pod_zone_name_sanitization() {
        // Dots get replaced with hyphens
        assert_eq!(
            pod_zone_name("my.namespace", "my.pod"),
            "reddwarf-my-namespace-my-pod"
        );
    }

    #[test]
    fn test_pod_zone_name_truncation() {
        let long_name = "a".repeat(60);
        let name = pod_zone_name("ns", &long_name);
        assert!(name.len() <= 64);
    }

    #[test]
    fn test_pod_to_zone_config_maps_containers() {
        let mut pod = Pod::default();
        pod.metadata.name = Some("test-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            containers: vec![
                Container {
                    name: "web".to_string(),
                    command: Some(vec!["nginx".to_string()]),
                    args: Some(vec!["-g".to_string(), "daemon off;".to_string()]),
                    ..Default::default()
                },
                Container {
                    name: "sidecar".to_string(),
                    command: Some(vec!["/bin/sh".to_string(), "-c".to_string()]),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let config = PodControllerConfig {
            node_name: "node1".to_string(),
            api_url: "http://127.0.0.1:6443".to_string(),
            zonepath_prefix: "/zones".to_string(),
            zfs_parent_dataset: "rpool/zones".to_string(),
            default_brand: ZoneBrand::Reddwarf,
            network: NetworkMode::Etherstub(EtherstubConfig {
                etherstub_name: "reddwarf0".to_string(),
                vnic_name: "vnic0".to_string(),
                ip_address: "10.0.0.2".to_string(),
                gateway: "10.0.0.1".to_string(),
            }),
        };

        let zone_config = pod_to_zone_config(&pod, &config).unwrap();

        assert_eq!(zone_config.zone_name, "reddwarf-default-test-pod");
        assert_eq!(zone_config.zonepath, "/zones/reddwarf-default-test-pod");
        assert_eq!(zone_config.processes.len(), 2);
        assert_eq!(zone_config.processes[0].name, "web");
        assert_eq!(
            zone_config.processes[0].command,
            vec!["nginx", "-g", "daemon off;"]
        );
        assert_eq!(zone_config.processes[1].name, "sidecar");
        assert_eq!(zone_config.processes[1].command, vec!["/bin/sh", "-c"]);
        assert_eq!(zone_config.brand, ZoneBrand::Reddwarf);
        assert_eq!(zone_config.zfs.parent_dataset, "rpool/zones");
    }

    #[test]
    fn test_pod_to_zone_config_no_spec_returns_error() {
        let mut pod = Pod::default();
        pod.metadata.name = Some("test-pod".to_string());
        // No spec set

        let config = PodControllerConfig {
            node_name: "node1".to_string(),
            api_url: "http://127.0.0.1:6443".to_string(),
            zonepath_prefix: "/zones".to_string(),
            zfs_parent_dataset: "rpool/zones".to_string(),
            default_brand: ZoneBrand::Reddwarf,
            network: NetworkMode::Etherstub(EtherstubConfig {
                etherstub_name: "reddwarf0".to_string(),
                vnic_name: "vnic0".to_string(),
                ip_address: "10.0.0.2".to_string(),
                gateway: "10.0.0.1".to_string(),
            }),
        };

        let result = pod_to_zone_config(&pod, &config);
        assert!(result.is_err());
    }
}
