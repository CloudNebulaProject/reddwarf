use crate::probes::executor::ProbeExecutor;
use crate::probes::types::{ContainerProbeConfig, ProbeKind, ProbeOutcome};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Composite key for per-probe state
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProbeKey {
    pod_key: String,
    container_name: String,
    kind: ProbeKind,
}

/// Per-probe mutable state
struct ProbeState {
    config: ContainerProbeConfig,
    container_started_at: Instant,
    last_check: Option<Instant>,
    consecutive_successes: u32,
    consecutive_failures: u32,
    has_succeeded: bool,
}

/// Aggregate probe status for a pod
#[derive(Debug, Clone)]
pub struct PodProbeStatus {
    /// All readiness probes pass (or none defined)
    pub ready: bool,
    /// Any liveness probe has failed past its failure threshold
    pub liveness_failed: bool,
    /// Diagnostic detail about the failure
    pub failure_message: Option<String>,
}

/// Tracks probe state for all pods and drives periodic checks
pub struct ProbeTracker {
    states: HashMap<ProbeKey, ProbeState>,
    executor: ProbeExecutor,
}

impl ProbeTracker {
    pub fn new(executor: ProbeExecutor) -> Self {
        Self {
            states: HashMap::new(),
            executor,
        }
    }

    /// Register (or re-register) probes for a pod. Idempotent — existing state
    /// is preserved if the probe key already exists.
    pub fn register_pod(
        &mut self,
        pod_key: &str,
        probes: Vec<ContainerProbeConfig>,
        started_at: Instant,
    ) {
        for config in probes {
            let key = ProbeKey {
                pod_key: pod_key.to_string(),
                container_name: config.container_name.clone(),
                kind: config.kind,
            };

            // Idempotent: don't overwrite existing tracking state
            self.states.entry(key).or_insert(ProbeState {
                config,
                container_started_at: started_at,
                last_check: None,
                consecutive_successes: 0,
                consecutive_failures: 0,
                has_succeeded: false,
            });
        }
    }

    /// Remove all probe state for a pod
    pub fn unregister_pod(&mut self, pod_key: &str) {
        self.states.retain(|k, _| k.pod_key != pod_key);
    }

    /// Run due probes for a pod and return its aggregate status
    pub async fn check_pod(
        &mut self,
        pod_key: &str,
        zone_name: &str,
        zone_ip: &str,
    ) -> PodProbeStatus {
        let now = Instant::now();

        // Collect keys for this pod
        let keys: Vec<ProbeKey> = self
            .states
            .keys()
            .filter(|k| k.pod_key == pod_key)
            .cloned()
            .collect();

        if keys.is_empty() {
            // No probes registered — pod is ready by default
            return PodProbeStatus {
                ready: true,
                liveness_failed: false,
                failure_message: None,
            };
        }

        // Check whether startup probes have succeeded (gates liveness)
        let startup_succeeded: HashMap<String, bool> = {
            let mut map = HashMap::new();
            for key in &keys {
                if key.kind == ProbeKind::Startup {
                    if let Some(state) = self.states.get(key) {
                        map.insert(key.container_name.clone(), state.has_succeeded);
                    }
                }
            }
            map
        };

        // Run probes
        for key in &keys {
            let state = match self.states.get(key) {
                Some(s) => s,
                None => continue,
            };

            // Skip liveness probes if startup probe hasn't succeeded yet
            if key.kind == ProbeKind::Liveness {
                if let Some(&startup_done) = startup_succeeded.get(&key.container_name) {
                    if !startup_done {
                        debug!(
                            "Skipping liveness probe for container '{}' — startup probe hasn't passed yet",
                            key.container_name
                        );
                        continue;
                    }
                }
            }

            // Check initial delay
            let elapsed_since_start = now.duration_since(state.container_started_at);
            if elapsed_since_start < Duration::from_secs(state.config.initial_delay_seconds as u64)
            {
                debug!(
                    "Skipping {} probe for container '{}' — initial delay not elapsed",
                    key.kind, key.container_name
                );
                continue;
            }

            // Check period
            if let Some(last) = state.last_check {
                let since_last = now.duration_since(last);
                if since_last < Duration::from_secs(state.config.period_seconds as u64) {
                    continue;
                }
            }

            // Execute the probe
            let timeout = Duration::from_secs(state.config.timeout_seconds as u64);
            let result = self
                .executor
                .execute(zone_name, zone_ip, &state.config.action, timeout)
                .await;

            // Update state
            let state = self.states.get_mut(key).unwrap();
            state.last_check = Some(now);

            match result.outcome {
                ProbeOutcome::Success => {
                    state.consecutive_successes += 1;
                    state.consecutive_failures = 0;
                    if state.consecutive_successes >= state.config.success_threshold {
                        state.has_succeeded = true;
                    }
                }
                ProbeOutcome::Failure(ref msg) | ProbeOutcome::Error(ref msg) => {
                    state.consecutive_failures += 1;
                    state.consecutive_successes = 0;
                    if state.consecutive_failures >= state.config.failure_threshold {
                        warn!(
                            "{} probe failed for container '{}': {} (failures: {}/{})",
                            key.kind,
                            key.container_name,
                            msg,
                            state.consecutive_failures,
                            state.config.failure_threshold
                        );
                    }
                }
            }
        }

        // Compute aggregate status
        let mut ready = true;
        let mut liveness_failed = false;
        let mut failure_message = None;

        for key in &keys {
            let state = match self.states.get(key) {
                Some(s) => s,
                None => continue,
            };

            match key.kind {
                ProbeKind::Readiness => {
                    if !state.has_succeeded
                        || state.consecutive_failures >= state.config.failure_threshold
                    {
                        ready = false;
                        if state.consecutive_failures >= state.config.failure_threshold {
                            failure_message = Some(format!(
                                "Readiness probe failed for container '{}' ({} consecutive failures)",
                                key.container_name, state.consecutive_failures
                            ));
                        }
                    }
                }
                ProbeKind::Liveness => {
                    if state.consecutive_failures >= state.config.failure_threshold {
                        liveness_failed = true;
                        failure_message = Some(format!(
                            "Liveness probe failed for container '{}' ({} consecutive failures)",
                            key.container_name, state.consecutive_failures
                        ));
                    }
                }
                ProbeKind::Startup => {
                    // Startup probe failure past threshold is treated as liveness failure
                    if !state.has_succeeded
                        && state.consecutive_failures >= state.config.failure_threshold
                    {
                        liveness_failed = true;
                        failure_message = Some(format!(
                            "Startup probe failed for container '{}' ({} consecutive failures)",
                            key.container_name, state.consecutive_failures
                        ));
                    }
                    // Also gate readiness on startup
                    if !state.has_succeeded {
                        ready = false;
                    }
                }
            }
        }

        PodProbeStatus {
            ready,
            liveness_failed,
            failure_message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandOutput;
    use crate::mock::MockRuntime;
    use crate::probes::types::{ContainerProbeConfig, ProbeAction, ProbeKind};
    use crate::storage::MockStorageEngine;
    use crate::traits::ZoneRuntime;
    use crate::types::{
        EtherstubConfig, NetworkMode, StoragePoolConfig, ZoneBrand, ZoneConfig, ZoneStorageOpts,
    };
    use std::sync::Arc;

    fn make_test_runtime() -> Arc<MockRuntime> {
        let storage = Arc::new(MockStorageEngine::new(StoragePoolConfig::from_pool("rpool")));
        Arc::new(MockRuntime::new(storage))
    }

    fn make_zone_config(name: &str) -> ZoneConfig {
        ZoneConfig {
            zone_name: name.to_string(),
            brand: ZoneBrand::Reddwarf,
            zonepath: format!("/zones/{}", name),
            network: NetworkMode::Etherstub(EtherstubConfig {
                etherstub_name: "reddwarf0".to_string(),
                vnic_name: format!("vnic_{}", name),
                ip_address: "10.0.0.2".to_string(),
                gateway: "10.0.0.1".to_string(),
                prefix_len: 16,
            }),
            storage: ZoneStorageOpts::default(),
            lx_image_path: None,
            processes: vec![],
            cpu_cap: None,
            memory_cap: None,
            fs_mounts: vec![],
        }
    }

    fn exec_probe_config(
        container: &str,
        kind: ProbeKind,
        failure_threshold: u32,
    ) -> ContainerProbeConfig {
        ContainerProbeConfig {
            container_name: container.to_string(),
            kind,
            action: ProbeAction::Exec {
                command: vec!["check".to_string()],
            },
            initial_delay_seconds: 0,
            period_seconds: 0, // Always due
            timeout_seconds: 5,
            failure_threshold,
            success_threshold: 1,
        }
    }

    #[tokio::test]
    async fn test_register_and_check_success() {
        let runtime = make_test_runtime();
        let config = make_zone_config("probe-ok");
        runtime.provision(&config).await.unwrap();

        let executor = ProbeExecutor::new(runtime.clone());
        let mut tracker = ProbeTracker::new(executor);

        let probes = vec![exec_probe_config("web", ProbeKind::Liveness, 3)];
        tracker.register_pod("default/probe-ok", probes, Instant::now());

        let status = tracker
            .check_pod("default/probe-ok", "probe-ok", "10.0.0.2")
            .await;
        assert!(!status.liveness_failed);
        assert!(status.ready); // No readiness probes → default ready
    }

    #[tokio::test]
    async fn test_liveness_failure_after_threshold() {
        let runtime = make_test_runtime();
        let config = make_zone_config("liveness-fail");
        runtime.provision(&config).await.unwrap();

        // Queue 3 failures (threshold is 3)
        for _ in 0..3 {
            runtime
                .set_exec_result(
                    "liveness-fail",
                    CommandOutput {
                        stdout: String::new(),
                        stderr: "unhealthy".to_string(),
                        exit_code: 1,
                    },
                )
                .await;
        }

        let executor = ProbeExecutor::new(runtime.clone());
        let mut tracker = ProbeTracker::new(executor);

        let probes = vec![exec_probe_config("web", ProbeKind::Liveness, 3)];
        tracker.register_pod("default/liveness-fail", probes, Instant::now());

        // Run probes 3 times to hit the threshold — the 3rd call reaches it
        let mut status = PodProbeStatus {
            ready: true,
            liveness_failed: false,
            failure_message: None,
        };
        for _ in 0..3 {
            status = tracker
                .check_pod("default/liveness-fail", "liveness-fail", "10.0.0.2")
                .await;
        }

        assert!(status.liveness_failed);
        assert!(status.failure_message.is_some());
    }

    #[tokio::test]
    async fn test_readiness_failure_sets_not_ready() {
        let runtime = make_test_runtime();
        let config = make_zone_config("readiness-fail");
        runtime.provision(&config).await.unwrap();

        // Queue failures
        for _ in 0..3 {
            runtime
                .set_exec_result(
                    "readiness-fail",
                    CommandOutput {
                        stdout: String::new(),
                        stderr: "not ready".to_string(),
                        exit_code: 1,
                    },
                )
                .await;
        }

        let executor = ProbeExecutor::new(runtime.clone());
        let mut tracker = ProbeTracker::new(executor);

        let probes = vec![exec_probe_config("web", ProbeKind::Readiness, 3)];
        tracker.register_pod("default/readiness-fail", probes, Instant::now());

        // Run probes 3 times — the 3rd call reaches the threshold
        let mut status = PodProbeStatus {
            ready: true,
            liveness_failed: false,
            failure_message: None,
        };
        for _ in 0..3 {
            status = tracker
                .check_pod("default/readiness-fail", "readiness-fail", "10.0.0.2")
                .await;
        }

        assert!(!status.ready);
        assert!(!status.liveness_failed); // Readiness failure doesn't kill the pod
    }

    #[tokio::test]
    async fn test_initial_delay_respected() {
        let runtime = make_test_runtime();
        let config = make_zone_config("delay-zone");
        runtime.provision(&config).await.unwrap();

        // Queue a failure — but probe should not run due to initial delay
        runtime
            .set_exec_result(
                "delay-zone",
                CommandOutput {
                    stdout: String::new(),
                    stderr: "fail".to_string(),
                    exit_code: 1,
                },
            )
            .await;

        let executor = ProbeExecutor::new(runtime.clone());
        let mut tracker = ProbeTracker::new(executor);

        let mut probe_cfg = exec_probe_config("web", ProbeKind::Liveness, 1);
        probe_cfg.initial_delay_seconds = 3600; // 1 hour delay — won't be reached

        tracker.register_pod("default/delay-zone", vec![probe_cfg], Instant::now());

        let status = tracker
            .check_pod("default/delay-zone", "delay-zone", "10.0.0.2")
            .await;
        // Probe should have been skipped, so no failure
        assert!(!status.liveness_failed);
    }

    #[tokio::test]
    async fn test_startup_gates_liveness() {
        let runtime = make_test_runtime();
        let config = make_zone_config("startup-gate");
        runtime.provision(&config).await.unwrap();

        // Startup will fail, liveness should be skipped
        runtime
            .set_exec_result(
                "startup-gate",
                CommandOutput {
                    stdout: String::new(),
                    stderr: "not started".to_string(),
                    exit_code: 1,
                },
            )
            .await;

        let executor = ProbeExecutor::new(runtime.clone());
        let mut tracker = ProbeTracker::new(executor);

        let probes = vec![
            ContainerProbeConfig {
                container_name: "web".to_string(),
                kind: ProbeKind::Startup,
                action: ProbeAction::Exec {
                    command: vec!["startup-check".to_string()],
                },
                initial_delay_seconds: 0,
                period_seconds: 0,
                timeout_seconds: 5,
                failure_threshold: 10, // High threshold so we don't fail yet
                success_threshold: 1,
            },
            exec_probe_config("web", ProbeKind::Liveness, 1),
        ];
        tracker.register_pod("default/startup-gate", probes, Instant::now());

        let status = tracker
            .check_pod("default/startup-gate", "startup-gate", "10.0.0.2")
            .await;
        // Startup hasn't succeeded → liveness should be skipped → no liveness failure
        assert!(!status.liveness_failed);
        // But pod is not ready (startup gate)
        assert!(!status.ready);
    }

    #[tokio::test]
    async fn test_unregister_cleans_state() {
        let runtime = make_test_runtime();
        let executor = ProbeExecutor::new(runtime.clone());
        let mut tracker = ProbeTracker::new(executor);

        let probes = vec![exec_probe_config("web", ProbeKind::Liveness, 3)];
        tracker.register_pod("default/cleanup-pod", probes, Instant::now());

        // Verify state exists
        assert!(!tracker.states.is_empty());

        tracker.unregister_pod("default/cleanup-pod");

        // State should be empty
        assert!(tracker.states.is_empty());
    }
}
