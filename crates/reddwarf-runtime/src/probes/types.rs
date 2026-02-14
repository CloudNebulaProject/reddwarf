use k8s_openapi::api::core::v1::Container;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use std::time::{Duration, Instant};

/// Which kind of probe this is
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeKind {
    Startup,
    Liveness,
    Readiness,
}

impl std::fmt::Display for ProbeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeKind::Startup => write!(f, "startup"),
            ProbeKind::Liveness => write!(f, "liveness"),
            ProbeKind::Readiness => write!(f, "readiness"),
        }
    }
}

/// The action a probe performs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeAction {
    Exec { command: Vec<String> },
    HttpGet { path: String, port: u16, host: String, scheme: String },
    TcpSocket { port: u16, host: String },
}

/// Extracted probe configuration for a single container + probe kind
#[derive(Debug, Clone)]
pub struct ContainerProbeConfig {
    pub container_name: String,
    pub kind: ProbeKind,
    pub action: ProbeAction,
    pub initial_delay_seconds: u32,
    pub period_seconds: u32,
    pub timeout_seconds: u32,
    pub failure_threshold: u32,
    pub success_threshold: u32,
}

/// Outcome of a single probe execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Success,
    Failure(String),
    Error(String),
}

/// Result of a probe execution with timing metadata
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub outcome: ProbeOutcome,
    pub duration: Duration,
    pub timestamp: Instant,
}

/// Resolve an IntOrString port to a u16.
/// Named ports are not supported (would require pod spec lookup); they return 0.
fn resolve_port(port: &IntOrString) -> u16 {
    match port {
        IntOrString::Int(n) => *n as u16,
        IntOrString::String(s) => s.parse::<u16>().unwrap_or(0),
    }
}

/// Extract all probe configs from a k8s Container
pub fn extract_probes(container: &Container) -> Vec<ContainerProbeConfig> {
    let mut probes = Vec::new();

    let probe_sources = [
        (&container.startup_probe, ProbeKind::Startup),
        (&container.liveness_probe, ProbeKind::Liveness),
        (&container.readiness_probe, ProbeKind::Readiness),
    ];

    for (probe_opt, kind) in probe_sources {
        let probe = match probe_opt {
            Some(p) => p,
            None => continue,
        };

        let action = if let Some(exec) = &probe.exec {
            match &exec.command {
                Some(cmd) if !cmd.is_empty() => ProbeAction::Exec {
                    command: cmd.clone(),
                },
                _ => continue, // Empty or missing exec command — skip
            }
        } else if let Some(http) = &probe.http_get {
            let port = resolve_port(&http.port);
            if port == 0 {
                continue;
            }
            ProbeAction::HttpGet {
                path: http.path.clone().unwrap_or_else(|| "/".to_string()),
                port,
                host: http.host.clone().unwrap_or_else(|| "localhost".to_string()),
                scheme: http.scheme.clone().unwrap_or_else(|| "HTTP".to_string()),
            }
        } else if let Some(tcp) = &probe.tcp_socket {
            let port = resolve_port(&tcp.port);
            if port == 0 {
                continue;
            }
            ProbeAction::TcpSocket {
                port,
                host: tcp.host.clone().unwrap_or_else(|| "localhost".to_string()),
            }
        } else {
            continue; // No recognized action
        };

        // Apply k8s defaults: period=10, timeout=1, failure=3, success=1, initial_delay=0
        probes.push(ContainerProbeConfig {
            container_name: container.name.clone(),
            kind,
            action,
            initial_delay_seconds: probe.initial_delay_seconds.unwrap_or(0) as u32,
            period_seconds: probe.period_seconds.unwrap_or(10) as u32,
            timeout_seconds: probe.timeout_seconds.unwrap_or(1) as u32,
            failure_threshold: probe.failure_threshold.unwrap_or(3) as u32,
            success_threshold: probe.success_threshold.unwrap_or(1) as u32,
        });
    }

    probes
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ExecAction, HTTPGetAction, Probe, TCPSocketAction,
    };

    #[test]
    fn test_extract_exec_probe() {
        let container = Container {
            name: "web".to_string(),
            liveness_probe: Some(Probe {
                exec: Some(ExecAction {
                    command: Some(vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()]),
                }),
                period_seconds: Some(5),
                failure_threshold: Some(2),
                ..Default::default()
            }),
            ..Default::default()
        };

        let probes = extract_probes(&container);
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].kind, ProbeKind::Liveness);
        assert_eq!(
            probes[0].action,
            ProbeAction::Exec {
                command: vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()]
            }
        );
        assert_eq!(probes[0].period_seconds, 5);
        assert_eq!(probes[0].failure_threshold, 2);
        // Defaults applied
        assert_eq!(probes[0].timeout_seconds, 1);
        assert_eq!(probes[0].success_threshold, 1);
        assert_eq!(probes[0].initial_delay_seconds, 0);
    }

    #[test]
    fn test_extract_http_probe() {
        let container = Container {
            name: "api".to_string(),
            readiness_probe: Some(Probe {
                http_get: Some(HTTPGetAction {
                    path: Some("/healthz".to_string()),
                    port: IntOrString::Int(8080),
                    host: Some("10.0.0.5".to_string()),
                    scheme: Some("HTTPS".to_string()),
                    ..Default::default()
                }),
                initial_delay_seconds: Some(15),
                ..Default::default()
            }),
            ..Default::default()
        };

        let probes = extract_probes(&container);
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].kind, ProbeKind::Readiness);
        assert_eq!(
            probes[0].action,
            ProbeAction::HttpGet {
                path: "/healthz".to_string(),
                port: 8080,
                host: "10.0.0.5".to_string(),
                scheme: "HTTPS".to_string(),
            }
        );
        assert_eq!(probes[0].initial_delay_seconds, 15);
    }

    #[test]
    fn test_extract_tcp_probe() {
        let container = Container {
            name: "db".to_string(),
            startup_probe: Some(Probe {
                tcp_socket: Some(TCPSocketAction {
                    port: IntOrString::Int(5432),
                    host: None,
                }),
                period_seconds: Some(2),
                failure_threshold: Some(30),
                ..Default::default()
            }),
            ..Default::default()
        };

        let probes = extract_probes(&container);
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].kind, ProbeKind::Startup);
        assert_eq!(
            probes[0].action,
            ProbeAction::TcpSocket {
                port: 5432,
                host: "localhost".to_string(),
            }
        );
        assert_eq!(probes[0].period_seconds, 2);
        assert_eq!(probes[0].failure_threshold, 30);
    }

    #[test]
    fn test_extract_no_probes() {
        let container = Container {
            name: "bare".to_string(),
            ..Default::default()
        };

        let probes = extract_probes(&container);
        assert!(probes.is_empty());
    }

    #[test]
    fn test_extract_defaults() {
        let container = Container {
            name: "defaults".to_string(),
            liveness_probe: Some(Probe {
                exec: Some(ExecAction {
                    command: Some(vec!["true".to_string()]),
                }),
                // All timing fields left as None → should get k8s defaults
                ..Default::default()
            }),
            ..Default::default()
        };

        let probes = extract_probes(&container);
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].initial_delay_seconds, 0);
        assert_eq!(probes[0].period_seconds, 10);
        assert_eq!(probes[0].timeout_seconds, 1);
        assert_eq!(probes[0].failure_threshold, 3);
        assert_eq!(probes[0].success_threshold, 1);
    }

    #[test]
    fn test_extract_empty_exec_command_skipped() {
        let container = Container {
            name: "empty-exec".to_string(),
            liveness_probe: Some(Probe {
                exec: Some(ExecAction {
                    command: Some(vec![]),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let probes = extract_probes(&container);
        assert!(probes.is_empty());
    }
}
