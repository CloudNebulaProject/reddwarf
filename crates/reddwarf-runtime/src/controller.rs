use crate::api_client::ApiClient;
use crate::error::{Result, RuntimeError};
use crate::network::{vnic_name_for_pod, Ipam};
use crate::probes::executor::ProbeExecutor;
use crate::probes::tracker::ProbeTracker;
use crate::probes::types::extract_probes;
use crate::traits::ZoneRuntime;
use crate::types::*;
use chrono::Utc;
use k8s_openapi::api::core::v1::{Pod, PodCondition, PodStatus};
use reddwarf_core::{ResourceEvent, ResourceQuantities, WatchEventType};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;
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
    /// Default zone brand
    pub default_brand: ZoneBrand,
    /// Name of the etherstub for pod networking
    pub etherstub_name: String,
    /// Pod CIDR (e.g., "10.88.0.0/16")
    pub pod_cidr: String,
    /// Interval between periodic full reconciliation cycles
    pub reconcile_interval: Duration,
}

/// Pod controller that watches for Pod events and drives zone lifecycle
pub struct PodController {
    runtime: Arc<dyn ZoneRuntime>,
    api_client: Arc<ApiClient>,
    event_tx: broadcast::Sender<ResourceEvent>,
    config: PodControllerConfig,
    ipam: Ipam,
    probe_tracker: Mutex<ProbeTracker>,
}

impl PodController {
    pub fn new(
        runtime: Arc<dyn ZoneRuntime>,
        api_client: Arc<ApiClient>,
        event_tx: broadcast::Sender<ResourceEvent>,
        config: PodControllerConfig,
        ipam: Ipam,
    ) -> Self {
        let probe_executor = ProbeExecutor::new(Arc::clone(&runtime));
        let probe_tracker = Mutex::new(ProbeTracker::new(probe_executor));
        Self {
            runtime,
            api_client,
            event_tx,
            config,
            ipam,
            probe_tracker,
        }
    }

    /// Run the controller — reacts to pod events from the in-process event bus.
    ///
    /// On startup, performs a full reconcile to catch up on any pods that were
    /// scheduled while the controller was down. Then switches to event-driven mode.
    pub async fn run(&self, token: CancellationToken) -> Result<()> {
        info!(
            "Starting pod controller for node '{}'",
            self.config.node_name
        );

        // Initial full sync
        if let Err(e) = self.reconcile_all().await {
            error!("Initial reconcile failed: {}", e);
        }

        let mut rx = self.event_tx.subscribe();
        let mut reconcile_tick = tokio::time::interval(self.config.reconcile_interval);
        // Consume the first tick — we just did reconcile_all() above
        reconcile_tick.tick().await;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("Pod controller shutting down");
                    return Ok(());
                }
                _ = reconcile_tick.tick() => {
                    debug!("Periodic reconcile tick");
                    if let Err(e) = self.reconcile_all().await {
                        error!("Periodic reconcile failed: {}", e);
                    }
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            if event.gvk.kind != "Pod" {
                                continue;
                            }
                            match event.event_type {
                                WatchEventType::Added | WatchEventType::Modified => {
                                    match serde_json::from_value::<Pod>(event.object) {
                                        Ok(pod) => {
                                            if let Err(e) = self.reconcile(&pod).await {
                                                let name = pod.metadata.name.as_deref().unwrap_or("<unknown>");
                                                error!("Failed to reconcile pod {}: {}", name, e);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to parse pod from event: {}", e);
                                        }
                                    }
                                }
                                WatchEventType::Deleted => {
                                    match serde_json::from_value::<Pod>(event.object) {
                                        Ok(pod) => {
                                            if let Err(e) = self.handle_delete(&pod).await {
                                                let name = pod.metadata.name.as_deref().unwrap_or("<unknown>");
                                                error!("Failed to handle pod deletion {}: {}", name, e);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to parse pod from delete event: {}", e);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("Missed {} events, doing full resync", n);
                            if let Err(e) = self.reconcile_all().await {
                                error!("Resync after lag failed: {}", e);
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Event bus closed, stopping pod controller");
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Reconcile all pods assigned to this node
    async fn reconcile_all(&self) -> Result<()> {
        debug!("Running pod controller reconcile cycle");

        // List all pods via the API client (respects TLS configuration)
        let body = self.api_client.get_json("/api/v1/pods").await?;

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

        // If the pod has a deletion_timestamp, drive the termination state machine
        if pod.metadata.deletion_timestamp.is_some() {
            return self.handle_termination(pod).await;
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
                let zone_config = self.pod_to_zone_config(pod)?;

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
                        // Zone is running — execute health probes
                        let pod_key = format!("{}/{}", namespace, pod_name);
                        let zone_ip = self.get_pod_ip(pod);

                        // Extract and register probes (idempotent)
                        let probes = self.extract_pod_probes(pod);
                        let started_at = self.pod_start_time(pod);

                        let mut tracker = self.probe_tracker.lock().await;
                        tracker.register_pod(&pod_key, probes, started_at);

                        let status = tracker
                            .check_pod(&pod_key, &zone_name, &zone_ip)
                            .await;
                        drop(tracker);

                        if status.liveness_failed {
                            let message = status.failure_message.unwrap_or_else(|| {
                                "Liveness probe failed".to_string()
                            });
                            warn!(
                                "Liveness probe failed for pod {}/{}: {}",
                                namespace, pod_name, message
                            );
                            let pod_status = PodStatus {
                                phase: Some("Failed".to_string()),
                                conditions: Some(vec![PodCondition {
                                    type_: "Ready".to_string(),
                                    status: "False".to_string(),
                                    reason: Some("LivenessProbeFailure".to_string()),
                                    message: Some(message),
                                    ..Default::default()
                                }]),
                                ..Default::default()
                            };

                            if let Err(e) = self
                                .api_client
                                .set_pod_status(namespace, pod_name, pod_status)
                                .await
                            {
                                error!("Failed to update pod status to Failed: {}", e);
                            }

                            // Unregister probes for this pod
                            let mut tracker = self.probe_tracker.lock().await;
                            tracker.unregister_pod(&pod_key);
                        } else if !status.ready {
                            let message = status.failure_message.unwrap_or_else(|| {
                                "Readiness probe failed".to_string()
                            });
                            debug!(
                                "Readiness probe not passing for pod {}/{}: {}",
                                namespace, pod_name, message
                            );

                            // Only update if currently marked Ready=True
                            let currently_ready = pod
                                .status
                                .as_ref()
                                .and_then(|s| s.conditions.as_ref())
                                .and_then(|c| c.iter().find(|c| c.type_ == "Ready"))
                                .map(|c| c.status == "True")
                                .unwrap_or(false);

                            if currently_ready {
                                let pod_status = PodStatus {
                                    phase: Some("Running".to_string()),
                                    conditions: Some(vec![PodCondition {
                                        type_: "Ready".to_string(),
                                        status: "False".to_string(),
                                        reason: Some("ReadinessProbeFailure".to_string()),
                                        message: Some(message),
                                        ..Default::default()
                                    }]),
                                    pod_ip: Some(zone_ip),
                                    ..Default::default()
                                };

                                if let Err(e) = self
                                    .api_client
                                    .set_pod_status(namespace, pod_name, pod_status)
                                    .await
                                {
                                    error!("Failed to update pod status: {}", e);
                                }
                            }
                        } else {
                            // All probes pass — set Ready=True if not already
                            let currently_ready = pod
                                .status
                                .as_ref()
                                .and_then(|s| s.conditions.as_ref())
                                .and_then(|c| c.iter().find(|c| c.type_ == "Ready"))
                                .map(|c| c.status == "True")
                                .unwrap_or(false);

                            if !currently_ready {
                                let pod_status = PodStatus {
                                    phase: Some("Running".to_string()),
                                    conditions: Some(vec![PodCondition {
                                        type_: "Ready".to_string(),
                                        status: "True".to_string(),
                                        ..Default::default()
                                    }]),
                                    pod_ip: Some(zone_ip),
                                    ..Default::default()
                                };

                                if let Err(e) = self
                                    .api_client
                                    .set_pod_status(namespace, pod_name, pod_status)
                                    .await
                                {
                                    error!("Failed to update pod status: {}", e);
                                }
                            }
                        }
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

    /// Handle pod deletion — deprovision the zone and release IP.
    ///
    /// If the pod has a `deletion_timestamp`, the graceful termination state
    /// machine (`handle_termination`) is responsible for cleanup, so this method
    /// becomes a no-op. Otherwise (e.g. a direct storage delete that bypasses
    /// the graceful path), fall back to the original immediate cleanup.
    pub async fn handle_delete(&self, pod: &Pod) -> Result<()> {
        let pod_name = pod
            .metadata
            .name
            .as_deref()
            .ok_or_else(|| RuntimeError::internal_error("Pod has no name"))?;
        let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");

        // If deletion_timestamp is set, handle_termination is driving cleanup
        if pod.metadata.deletion_timestamp.is_some() {
            debug!(
                "Pod {}/{} has deletion_timestamp, skipping handle_delete (handled by termination state machine)",
                namespace, pod_name
            );
            return Ok(());
        }

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

        let zone_config = self.pod_to_zone_config(pod)?;
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

        // Release the IP allocation
        if let Err(e) = self.ipam.release(namespace, pod_name) {
            warn!(
                "Failed to release IP for pod {}/{}: {}",
                namespace, pod_name, e
            );
        }

        // Unregister probes
        let pod_key = format!("{}/{}", namespace, pod_name);
        let mut tracker = self.probe_tracker.lock().await;
        tracker.unregister_pod(&pod_key);

        Ok(())
    }

    /// Drive the graceful termination state machine for a pod with
    /// `deletion_timestamp` set.
    ///
    /// | Zone State      | Grace Expired? | Action                                     |
    /// |-----------------|----------------|--------------------------------------------|
    /// | Running         | No             | shutdown_zone() (graceful)                 |
    /// | Running         | Yes            | halt_zone() (force kill)                   |
    /// | ShuttingDown    | No             | Wait (next reconcile will re-check)        |
    /// | ShuttingDown    | Yes            | halt_zone() (force kill)                   |
    /// | Stopped/Absent  | —              | deprovision(), release IP, finalize_pod()  |
    async fn handle_termination(&self, pod: &Pod) -> Result<()> {
        let pod_name = pod
            .metadata
            .name
            .as_deref()
            .ok_or_else(|| RuntimeError::internal_error("Pod has no name"))?;
        let namespace = pod.metadata.namespace.as_deref().unwrap_or("default");
        let zone_name = pod_zone_name(namespace, pod_name);

        let grace_expired = self.is_grace_period_expired(pod);

        // Query actual zone state
        let zone_state = match self.runtime.get_zone_state(&zone_name).await {
            Ok(state) => state,
            Err(RuntimeError::ZoneNotFound { .. }) => ZoneState::Absent,
            Err(e) => {
                warn!(
                    "Could not check zone state for {}: {} — treating as absent",
                    zone_name, e
                );
                ZoneState::Absent
            }
        };

        debug!(
            "Termination state machine for pod {}/{}: zone={}, grace_expired={}",
            namespace, pod_name, zone_state, grace_expired
        );

        match zone_state {
            ZoneState::Running => {
                if grace_expired {
                    warn!(
                        "Grace period expired for pod {}/{}, force halting zone {}",
                        namespace, pod_name, zone_name
                    );
                    if let Err(e) = self.runtime.halt_zone(&zone_name).await {
                        warn!("Failed to halt zone {}: {}", zone_name, e);
                    }
                    // Deprovision will happen on next reconcile when zone is stopped
                } else {
                    info!(
                        "Initiating graceful shutdown for zone {} (pod {}/{})",
                        zone_name, namespace, pod_name
                    );
                    if let Err(e) = self.runtime.shutdown_zone(&zone_name).await {
                        warn!("Failed to shut down zone {}: {}", zone_name, e);
                    }
                    // Next reconcile will re-check the zone state
                }
            }
            ZoneState::ShuttingDown => {
                if grace_expired {
                    warn!(
                        "Grace period expired while zone {} was shutting down, force halting",
                        zone_name
                    );
                    if let Err(e) = self.runtime.halt_zone(&zone_name).await {
                        warn!("Failed to halt zone {}: {}", zone_name, e);
                    }
                } else {
                    debug!(
                        "Zone {} is gracefully shutting down, waiting for next reconcile",
                        zone_name
                    );
                }
            }
            // Zone is stopped or absent — full cleanup
            _ => {
                info!(
                    "Zone {} is stopped/absent, cleaning up pod {}/{}",
                    zone_name, namespace, pod_name
                );

                // Try to deprovision (cleans up datasets, network, etc.)
                // Build a minimal zone config for deprovision — use existing IP if allocated
                let zone_config_result = self.pod_to_zone_config(pod);
                if let Ok(zone_config) = zone_config_result {
                    if let Err(e) = self.runtime.deprovision(&zone_config).await {
                        // Zone may already be fully cleaned up — that's fine
                        debug!(
                            "Deprovision for zone {} returned error (may be expected): {}",
                            zone_name, e
                        );
                    }
                }

                // Release the IP allocation
                if let Err(e) = self.ipam.release(namespace, pod_name) {
                    debug!(
                        "Failed to release IP for pod {}/{}: {} (may already be released)",
                        namespace, pod_name, e
                    );
                }

                // Unregister probes
                let pod_key = format!("{}/{}", namespace, pod_name);
                let mut tracker = self.probe_tracker.lock().await;
                tracker.unregister_pod(&pod_key);
                drop(tracker);

                // Finalize — remove the pod from API server storage
                if let Err(e) = self.api_client.finalize_pod(namespace, pod_name).await {
                    error!(
                        "Failed to finalize pod {}/{}: {}",
                        namespace, pod_name, e
                    );
                } else {
                    info!("Pod {}/{} finalized and removed", namespace, pod_name);
                }
            }
        }

        Ok(())
    }

    /// Check whether the pod's grace period has expired
    fn is_grace_period_expired(&self, pod: &Pod) -> bool {
        let deletion_ts = match &pod.metadata.deletion_timestamp {
            Some(t) => t.0,
            None => return false,
        };

        let grace_secs = pod
            .metadata
            .deletion_grace_period_seconds
            .unwrap_or(30);

        let deadline = deletion_ts + chrono::Duration::seconds(grace_secs);
        Utc::now() >= deadline
    }

    /// Convert a Pod spec to a ZoneConfig with per-pod VNIC and IP
    fn pod_to_zone_config(&self, pod: &Pod) -> Result<ZoneConfig> {
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
        let zonepath = format!("{}/{}", self.config.zonepath_prefix, zone_name);

        // Allocate a unique VNIC name and IP for this pod
        let vnic_name = vnic_name_for_pod(namespace, pod_name);
        let allocation = self.ipam.allocate(namespace, pod_name)?;

        let network = NetworkMode::Etherstub(EtherstubConfig {
            etherstub_name: self.config.etherstub_name.clone(),
            vnic_name,
            ip_address: allocation.ip_address.to_string(),
            gateway: allocation.gateway.to_string(),
            prefix_len: allocation.prefix_len,
        });

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

        // Aggregate resource limits across all containers in the pod.
        // Prefer limits (hard cap) over requests (soft guarantee).
        let (total_cpu_millicores, total_memory_bytes) =
            spec.containers.iter().fold((0i64, 0i64), |(cpu, mem), c| {
                let resources = c.resources.as_ref();
                let res_map = resources
                    .and_then(|r| r.limits.as_ref())
                    .or_else(|| resources.and_then(|r| r.requests.as_ref()));

                let (c_cpu, c_mem) = match res_map {
                    Some(map) => {
                        let rq = ResourceQuantities::from_k8s_resource_map(map);
                        (rq.cpu_millicores, rq.memory_bytes)
                    }
                    None => (0, 0),
                };
                (cpu + c_cpu, mem + c_mem)
            });

        let cpu_cap = if total_cpu_millicores > 0 {
            Some(ResourceQuantities::cpu_as_zone_cap(total_cpu_millicores))
        } else {
            None
        };

        let memory_cap = if total_memory_bytes > 0 {
            Some(ResourceQuantities::memory_as_zone_cap(total_memory_bytes))
        } else {
            None
        };

        let brand = pod
            .metadata
            .annotations
            .as_ref()
            .and_then(|a| a.get("reddwarf.io/zone-brand"))
            .and_then(|v| match v.as_str() {
                "lx" => Some(ZoneBrand::Lx),
                "reddwarf" => Some(ZoneBrand::Reddwarf),
                _ => None,
            })
            .unwrap_or_else(|| self.config.default_brand.clone());

        Ok(ZoneConfig {
            zone_name,
            brand,
            zonepath,
            network,
            storage: ZoneStorageOpts::default(),
            lx_image_path: None,
            processes,
            cpu_cap,
            memory_cap,
            fs_mounts: vec![],
        })
    }

    /// Extract IP address from zone config network
    fn zone_ip(&self, config: &ZoneConfig) -> String {
        match &config.network {
            NetworkMode::Etherstub(e) => e.ip_address.clone(),
            NetworkMode::Direct(d) => d.ip_address.clone(),
        }
    }

    /// Extract probe configurations from all containers in a pod spec
    fn extract_pod_probes(&self, pod: &Pod) -> Vec<crate::probes::types::ContainerProbeConfig> {
        let spec = match &pod.spec {
            Some(s) => s,
            None => return vec![],
        };

        spec.containers
            .iter()
            .flat_map(|c| extract_probes(c))
            .collect()
    }

    /// Get the pod's IP from its status, falling back to empty string
    fn get_pod_ip(&self, pod: &Pod) -> String {
        pod.status
            .as_ref()
            .and_then(|s| s.pod_ip.clone())
            .unwrap_or_default()
    }

    /// Approximate when the pod's containers started.
    /// Uses the pod's start_time if available, otherwise uses now.
    fn pod_start_time(&self, pod: &Pod) -> Instant {
        // We can't convert k8s Time to std Instant directly. Instead, compute
        // the elapsed duration since start_time and subtract from Instant::now().
        if let Some(start_time) = pod
            .status
            .as_ref()
            .and_then(|s| s.start_time.as_ref())
        {
            let now_utc = Utc::now();
            let started_utc = start_time.0;
            let elapsed = now_utc.signed_duration_since(started_utc);
            if let Ok(elapsed_std) = elapsed.to_std() {
                return Instant::now().checked_sub(elapsed_std).unwrap_or_else(Instant::now);
            }
        }
        Instant::now()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Ipam;
    use k8s_openapi::api::core::v1::{Container, PodSpec};
    use reddwarf_storage::RedbBackend;
    use std::net::Ipv4Addr;
    use std::time::Duration;
    use tempfile::tempdir;

    fn make_test_controller() -> (PodController, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test-controller.redb");
        let storage = Arc::new(RedbBackend::new(&db_path).unwrap());
        let ipam = Ipam::new(storage, "10.88.0.0/16").unwrap();

        let mock_storage = Arc::new(crate::storage::MockStorageEngine::new(
            crate::types::StoragePoolConfig::from_pool("rpool"),
        ));
        let runtime = Arc::new(crate::mock::MockRuntime::new(mock_storage));
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let (event_tx, _) = broadcast::channel(16);

        let config = PodControllerConfig {
            node_name: "node1".to_string(),
            api_url: "http://127.0.0.1:6443".to_string(),
            zonepath_prefix: "/zones".to_string(),
            default_brand: ZoneBrand::Reddwarf,
            etherstub_name: "reddwarf0".to_string(),
            pod_cidr: "10.88.0.0/16".to_string(),
            reconcile_interval: Duration::from_secs(30),
        };

        let controller = PodController::new(runtime, api_client, event_tx, config, ipam);
        (controller, dir)
    }

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
        let (controller, _dir) = make_test_controller();

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

        let zone_config = controller.pod_to_zone_config(&pod).unwrap();

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

        // Verify per-pod networking
        match &zone_config.network {
            NetworkMode::Etherstub(cfg) => {
                assert_eq!(cfg.etherstub_name, "reddwarf0");
                assert_eq!(cfg.vnic_name, "vnic_default_test_pod");
                assert_eq!(cfg.ip_address, Ipv4Addr::new(10, 88, 0, 2).to_string());
                assert_eq!(cfg.gateway, Ipv4Addr::new(10, 88, 0, 1).to_string());
                assert_eq!(cfg.prefix_len, 16);
            }
            _ => panic!("Expected Etherstub network mode"),
        }
    }

    #[test]
    fn test_pod_to_zone_config_unique_ips() {
        let (controller, _dir) = make_test_controller();

        let mut pod_a = Pod::default();
        pod_a.metadata.name = Some("pod-a".to_string());
        pod_a.metadata.namespace = Some("default".to_string());
        pod_a.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        let mut pod_b = Pod::default();
        pod_b.metadata.name = Some("pod-b".to_string());
        pod_b.metadata.namespace = Some("default".to_string());
        pod_b.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        let config_a = controller.pod_to_zone_config(&pod_a).unwrap();
        let config_b = controller.pod_to_zone_config(&pod_b).unwrap();

        let ip_a = match &config_a.network {
            NetworkMode::Etherstub(cfg) => cfg.ip_address.clone(),
            _ => panic!("Expected Etherstub"),
        };
        let ip_b = match &config_b.network {
            NetworkMode::Etherstub(cfg) => cfg.ip_address.clone(),
            _ => panic!("Expected Etherstub"),
        };

        assert_ne!(ip_a, ip_b, "Each pod should get a unique IP");
        assert_eq!(ip_a, "10.88.0.2");
        assert_eq!(ip_b, "10.88.0.3");
    }

    #[test]
    fn test_pod_to_zone_config_no_spec_returns_error() {
        let (controller, _dir) = make_test_controller();

        let mut pod = Pod::default();
        pod.metadata.name = Some("test-pod".to_string());
        // No spec set

        let result = controller.pod_to_zone_config(&pod);
        assert!(result.is_err());
    }

    #[test]
    fn test_pod_to_zone_config_with_cpu_and_memory_limits() {
        use k8s_openapi::api::core::v1::ResourceRequirements;
        use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
        use std::collections::BTreeMap;

        let (controller, _dir) = make_test_controller();

        let mut limits = BTreeMap::new();
        limits.insert("cpu".to_string(), Quantity("1".to_string()));
        limits.insert("memory".to_string(), Quantity("512Mi".to_string()));

        let mut pod = Pod::default();
        pod.metadata.name = Some("capped-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                resources: Some(ResourceRequirements {
                    limits: Some(limits),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        });

        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        assert_eq!(zone_config.cpu_cap, Some("1.00".to_string()));
        assert_eq!(zone_config.memory_cap, Some("512M".to_string()));
    }

    #[test]
    fn test_pod_to_zone_config_with_requests_fallback() {
        use k8s_openapi::api::core::v1::ResourceRequirements;
        use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
        use std::collections::BTreeMap;

        let (controller, _dir) = make_test_controller();

        let mut requests = BTreeMap::new();
        requests.insert("cpu".to_string(), Quantity("500m".to_string()));
        requests.insert("memory".to_string(), Quantity("256Mi".to_string()));

        let mut pod = Pod::default();
        pod.metadata.name = Some("req-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                resources: Some(ResourceRequirements {
                    requests: Some(requests),
                    limits: None,
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        });

        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        assert_eq!(zone_config.cpu_cap, Some("0.50".to_string()));
        assert_eq!(zone_config.memory_cap, Some("256M".to_string()));
    }

    #[test]
    fn test_pod_to_zone_config_aggregates_multiple_containers() {
        use k8s_openapi::api::core::v1::ResourceRequirements;
        use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
        use std::collections::BTreeMap;

        let (controller, _dir) = make_test_controller();

        let make_limits = |cpu: &str, mem: &str| {
            let mut limits = BTreeMap::new();
            limits.insert("cpu".to_string(), Quantity(cpu.to_string()));
            limits.insert("memory".to_string(), Quantity(mem.to_string()));
            limits
        };

        let mut pod = Pod::default();
        pod.metadata.name = Some("multi-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            containers: vec![
                Container {
                    name: "web".to_string(),
                    command: Some(vec!["/bin/sh".to_string()]),
                    resources: Some(ResourceRequirements {
                        limits: Some(make_limits("500m", "256Mi")),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Container {
                    name: "sidecar".to_string(),
                    command: Some(vec!["/bin/sh".to_string()]),
                    resources: Some(ResourceRequirements {
                        limits: Some(make_limits("500m", "256Mi")),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        // 500m + 500m = 1000m = 1.00
        assert_eq!(zone_config.cpu_cap, Some("1.00".to_string()));
        // 256Mi + 256Mi = 512Mi
        assert_eq!(zone_config.memory_cap, Some("512M".to_string()));
    }

    #[test]
    fn test_pod_to_zone_config_no_resources() {
        let (controller, _dir) = make_test_controller();

        let mut pod = Pod::default();
        pod.metadata.name = Some("bare-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        assert_eq!(zone_config.cpu_cap, None);
        assert_eq!(zone_config.memory_cap, None);
    }

    #[test]
    fn test_grace_period_not_expired() {
        let (controller, _dir) = make_test_controller();

        let mut pod = Pod::default();
        pod.metadata.name = Some("grace-pod".to_string());
        pod.metadata.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Utc::now()),
        );
        pod.metadata.deletion_grace_period_seconds = Some(30);

        assert!(!controller.is_grace_period_expired(&pod));
    }

    #[test]
    fn test_grace_period_expired() {
        let (controller, _dir) = make_test_controller();

        let mut pod = Pod::default();
        pod.metadata.name = Some("expired-pod".to_string());
        // Set deletion_timestamp 60 seconds in the past
        pod.metadata.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                Utc::now() - chrono::Duration::seconds(60),
            ),
        );
        pod.metadata.deletion_grace_period_seconds = Some(30);

        assert!(controller.is_grace_period_expired(&pod));
    }

    #[test]
    fn test_grace_period_no_deletion_timestamp() {
        let (controller, _dir) = make_test_controller();

        let pod = Pod::default();
        assert!(!controller.is_grace_period_expired(&pod));
    }

    #[tokio::test]
    async fn test_handle_termination_absent_zone_calls_finalize() {
        let (controller, _dir) = make_test_controller();

        // Build a pod with deletion_timestamp that is assigned to our node
        let mut pod = Pod::default();
        pod.metadata.name = Some("term-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.metadata.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Utc::now()),
        );
        pod.metadata.deletion_grace_period_seconds = Some(30);
        pod.spec = Some(PodSpec {
            node_name: Some("node1".to_string()),
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        // Zone doesn't exist in the mock runtime → should be treated as Absent
        // handle_termination will try to deprovision (fail, that's ok) and then
        // finalize (will fail since no API server, but the path is exercised)
        let result = controller.handle_termination(&pod).await;
        // The function should complete Ok (finalize failure is logged, not returned)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_termination_running_zone_graceful_shutdown() {
        let (controller, _dir) = make_test_controller();

        // First, provision a zone so it's Running
        let mut pod = Pod::default();
        pod.metadata.name = Some("running-term".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            node_name: Some("node1".to_string()),
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        // Provision the zone through the runtime
        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        controller.runtime.provision(&zone_config).await.unwrap();

        // Verify it's running
        let zone_name = pod_zone_name("default", "running-term");
        let state = controller.runtime.get_zone_state(&zone_name).await.unwrap();
        assert_eq!(state, ZoneState::Running);

        // Set deletion_timestamp (grace period NOT expired)
        pod.metadata.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Utc::now()),
        );
        pod.metadata.deletion_grace_period_seconds = Some(30);

        // Handle termination — should call shutdown_zone
        let result = controller.handle_termination(&pod).await;
        assert!(result.is_ok());

        // After shutdown, MockRuntime transitions zone to Installed
        let state = controller.runtime.get_zone_state(&zone_name).await.unwrap();
        assert_eq!(state, ZoneState::Installed);
    }

    #[tokio::test]
    async fn test_handle_delete_skips_when_deletion_timestamp_set() {
        let (controller, _dir) = make_test_controller();

        let mut pod = Pod::default();
        pod.metadata.name = Some("skip-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.metadata.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Utc::now()),
        );
        pod.spec = Some(PodSpec {
            node_name: Some("node1".to_string()),
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        // handle_delete should return Ok immediately — skipping deprovision
        let result = controller.handle_delete(&pod).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_pod_to_zone_config_brand_from_annotation() {
        let (controller, _dir) = make_test_controller();

        let mut pod = Pod::default();
        pod.metadata.name = Some("lx-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.metadata.annotations = Some(
            [("reddwarf.io/zone-brand".to_string(), "lx".to_string())]
                .into_iter()
                .collect(),
        );
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        assert_eq!(zone_config.brand, ZoneBrand::Lx);
    }

    #[test]
    fn test_pod_to_zone_config_brand_default() {
        let (controller, _dir) = make_test_controller();

        let mut pod = Pod::default();
        pod.metadata.name = Some("default-brand-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        // No annotations
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });

        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        assert_eq!(zone_config.brand, ZoneBrand::Reddwarf);
    }

    fn make_test_controller_with_runtime() -> (PodController, Arc<crate::mock::MockRuntime>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test-controller-rt.redb");
        let storage = Arc::new(RedbBackend::new(&db_path).unwrap());
        let ipam = Ipam::new(storage, "10.88.0.0/16").unwrap();

        let mock_storage = Arc::new(crate::storage::MockStorageEngine::new(
            crate::types::StoragePoolConfig::from_pool("rpool"),
        ));
        let runtime = Arc::new(crate::mock::MockRuntime::new(mock_storage));
        let api_client = Arc::new(ApiClient::new("http://127.0.0.1:6443"));
        let (event_tx, _) = broadcast::channel(16);

        let config = PodControllerConfig {
            node_name: "node1".to_string(),
            api_url: "http://127.0.0.1:6443".to_string(),
            zonepath_prefix: "/zones".to_string(),
            default_brand: ZoneBrand::Reddwarf,
            etherstub_name: "reddwarf0".to_string(),
            pod_cidr: "10.88.0.0/16".to_string(),
            reconcile_interval: Duration::from_secs(30),
        };

        let controller = PodController::new(runtime.clone() as Arc<dyn ZoneRuntime>, api_client, event_tx, config, ipam);
        (controller, runtime, dir)
    }

    #[tokio::test]
    async fn test_reconcile_running_pod_with_no_probes() {
        let (controller, runtime, _dir) = make_test_controller_with_runtime();

        // Create a pod that is already Running with a provisioned zone
        let mut pod = Pod::default();
        pod.metadata.name = Some("no-probe-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            node_name: Some("node1".to_string()),
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });
        pod.status = Some(PodStatus {
            phase: Some("Running".to_string()),
            pod_ip: Some("10.88.0.2".to_string()),
            conditions: Some(vec![PodCondition {
                type_: "Ready".to_string(),
                status: "True".to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        });

        // Provision the zone so get_zone_state returns Running
        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        runtime.provision(&zone_config).await.unwrap();

        // Reconcile — should succeed without changing anything (no probes)
        let result = controller.reconcile(&pod).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_reconcile_running_pod_liveness_failure() {
        let (controller, runtime, _dir) = make_test_controller_with_runtime();

        let mut pod = Pod::default();
        pod.metadata.name = Some("liveness-pod".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.spec = Some(PodSpec {
            node_name: Some("node1".to_string()),
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                liveness_probe: Some(k8s_openapi::api::core::v1::Probe {
                    exec: Some(k8s_openapi::api::core::v1::ExecAction {
                        command: Some(vec!["healthcheck".to_string()]),
                    }),
                    period_seconds: Some(0), // Always run
                    failure_threshold: Some(3),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        });
        pod.status = Some(PodStatus {
            phase: Some("Running".to_string()),
            pod_ip: Some("10.88.0.2".to_string()),
            conditions: Some(vec![PodCondition {
                type_: "Ready".to_string(),
                status: "True".to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        });

        // Provision the zone
        let zone_config = controller.pod_to_zone_config(&pod).unwrap();
        runtime.provision(&zone_config).await.unwrap();

        // Queue failures for the liveness probe
        let zone_name = pod_zone_name("default", "liveness-pod");
        for _ in 0..3 {
            runtime
                .set_exec_result(
                    &zone_name,
                    crate::command::CommandOutput {
                        stdout: String::new(),
                        stderr: "unhealthy".to_string(),
                        exit_code: 1,
                    },
                )
                .await;
        }

        // Reconcile 3 times to hit the failure threshold.
        // On the 3rd reconcile, liveness failure is detected. The controller
        // then unregisters the probes and tries to set the pod status to Failed
        // (which fails silently since there's no API server).
        for _ in 0..3 {
            let _ = controller.reconcile(&pod).await;
        }

        // Verify the liveness failure path was taken: probes should be unregistered
        let pod_key = "default/liveness-pod";
        let mut tracker = controller.probe_tracker.lock().await;
        let status = tracker
            .check_pod(pod_key, &zone_name, "10.88.0.2")
            .await;
        // No probes registered → default status (ready=true, liveness_failed=false)
        // This confirms the unregister happened, which only occurs on liveness failure
        assert!(status.ready);
        assert!(!status.liveness_failed);
    }

    #[tokio::test]
    async fn test_reconcile_with_deletion_timestamp_uses_termination() {
        let (controller, _dir) = make_test_controller();

        // Build a pod with deletion_timestamp, assigned to our node, phase Terminating
        let mut pod = Pod::default();
        pod.metadata.name = Some("recon-term".to_string());
        pod.metadata.namespace = Some("default".to_string());
        pod.metadata.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Utc::now()),
        );
        pod.metadata.deletion_grace_period_seconds = Some(30);
        pod.spec = Some(PodSpec {
            node_name: Some("node1".to_string()),
            containers: vec![Container {
                name: "web".to_string(),
                command: Some(vec!["/bin/sh".to_string()]),
                ..Default::default()
            }],
            ..Default::default()
        });
        pod.status = Some(PodStatus {
            phase: Some("Terminating".to_string()),
            ..Default::default()
        });

        // reconcile() should detect deletion_timestamp and call handle_termination
        // Zone is absent → will try to finalize (will fail, but logged)
        let result = controller.reconcile(&pod).await;
        assert!(result.is_ok());
    }
}
