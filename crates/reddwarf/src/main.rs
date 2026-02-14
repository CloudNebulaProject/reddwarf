use clap::{Parser, Subcommand};
use reddwarf_apiserver::{ApiError, ApiServer, AppState, Config as ApiConfig, TlsMode};
use reddwarf_core::{Namespace, ResourceQuantities};
use reddwarf_runtime::{
    ApiClient, Ipam, MockRuntime, MockStorageEngine, NodeAgent, NodeAgentConfig,
    NodeHealthChecker, NodeHealthCheckerConfig, PodController, PodControllerConfig, StorageEngine,
    StoragePoolConfig, ZoneBrand,
};
use reddwarf_scheduler::scheduler::SchedulerConfig;
use reddwarf_scheduler::Scheduler;
use reddwarf_storage::RedbBackend;
use reddwarf_versioning::VersionStore;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "reddwarf", about = "Reddwarf Kubernetes Control Plane")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Shared TLS arguments for both `serve` and `agent` subcommands.
#[derive(clap::Args, Clone, Debug)]
struct TlsArgs {
    /// Enable TLS (HTTPS). When set without --tls-cert/--tls-key, a
    /// self-signed CA + server certificate is auto-generated.
    #[arg(long, default_value_t = false)]
    tls: bool,

    /// Path to a PEM-encoded TLS certificate (requires --tls)
    #[arg(long, requires = "tls")]
    tls_cert: Option<String>,

    /// Path to a PEM-encoded TLS private key (requires --tls)
    #[arg(long, requires = "tls")]
    tls_key: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the API server only
    Serve {
        /// Address to listen on
        #[arg(long, default_value = "0.0.0.0:6443")]
        bind: String,
        /// Path to the redb database file
        #[arg(long, default_value = "./reddwarf.redb")]
        data_dir: String,
        #[command(flatten)]
        tls_args: TlsArgs,
    },
    /// Run as a full node agent (API server + scheduler + controller + heartbeat)
    Agent {
        /// Node name to register as
        #[arg(long)]
        node_name: String,
        /// Address to listen on
        #[arg(long, default_value = "0.0.0.0:6443")]
        bind: String,
        /// Path to the redb database file
        #[arg(long, default_value = "./reddwarf.redb")]
        data_dir: String,
        /// Base ZFS storage pool name (auto-derives {pool}/zones, {pool}/images, {pool}/volumes)
        #[arg(long, default_value = "rpool")]
        storage_pool: String,
        /// Override the zones dataset (default: {storage_pool}/zones)
        #[arg(long)]
        zones_dataset: Option<String>,
        /// Override the images dataset (default: {storage_pool}/images)
        #[arg(long)]
        images_dataset: Option<String>,
        /// Override the volumes dataset (default: {storage_pool}/volumes)
        #[arg(long)]
        volumes_dataset: Option<String>,
        /// Prefix for zone root paths (default: derived from storage pool as "/{pool}/zones")
        #[arg(long)]
        zonepath_prefix: Option<String>,
        /// Pod network CIDR for IPAM allocation
        #[arg(long, default_value = "10.88.0.0/16")]
        pod_cidr: String,
        /// Etherstub name for pod networking
        #[arg(long, default_value = "reddwarf0")]
        etherstub_name: String,
        /// CPU to reserve for system daemons (e.g. "100m", "0.1")
        #[arg(long, default_value = "100m")]
        system_reserved_cpu: String,
        /// Memory to reserve for system daemons (e.g. "256Mi", "1Gi")
        #[arg(long, default_value = "256Mi")]
        system_reserved_memory: String,
        /// Maximum number of pods this node will accept
        #[arg(long, default_value_t = 110)]
        max_pods: u32,
        #[command(flatten)]
        tls_args: TlsArgs,
    },
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            bind,
            data_dir,
            tls_args,
        } => run_serve(&bind, &data_dir, &tls_args).await,
        Commands::Agent {
            node_name,
            bind,
            data_dir,
            storage_pool,
            zones_dataset,
            images_dataset,
            volumes_dataset,
            zonepath_prefix,
            pod_cidr,
            etherstub_name,
            system_reserved_cpu,
            system_reserved_memory,
            max_pods,
            tls_args,
        } => {
            let reserved_cpu_millicores =
                ResourceQuantities::parse_cpu(&system_reserved_cpu).map_err(|e| {
                    miette::miette!(
                        help = "Use a value like '100m' or '0.1' for --system-reserved-cpu",
                        "Invalid --system-reserved-cpu '{}': {}",
                        system_reserved_cpu,
                        e
                    )
                })?;
            let reserved_memory_bytes =
                ResourceQuantities::parse_memory(&system_reserved_memory).map_err(|e| {
                    miette::miette!(
                        help = "Use a value like '256Mi' or '1Gi' for --system-reserved-memory",
                        "Invalid --system-reserved-memory '{}': {}",
                        system_reserved_memory,
                        e
                    )
                })?;

            run_agent(
                &node_name,
                &bind,
                &data_dir,
                &storage_pool,
                zones_dataset.as_deref(),
                images_dataset.as_deref(),
                volumes_dataset.as_deref(),
                zonepath_prefix.as_deref(),
                &pod_cidr,
                &etherstub_name,
                reserved_cpu_millicores,
                reserved_memory_bytes,
                max_pods,
                &tls_args,
            )
            .await
        }
    }
}

/// Wait for either SIGINT (ctrl-c) or SIGTERM, returning which one fired.
async fn shutdown_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => "SIGINT",
        _ = sigterm.recv() => "SIGTERM",
    }
}

/// Derive a `TlsMode` from CLI arguments.
fn tls_mode_from_args(args: &TlsArgs, data_dir: &str) -> miette::Result<TlsMode> {
    if !args.tls {
        return Ok(TlsMode::Disabled);
    }

    match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => Ok(TlsMode::Provided {
            cert_path: PathBuf::from(cert),
            key_path: PathBuf::from(key),
        }),
        (None, None) => {
            let parent = PathBuf::from(data_dir)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf();
            Ok(TlsMode::AutoGenerate {
                data_dir: parent.join("tls"),
                san_entries: vec!["localhost".to_string(), "127.0.0.1".to_string()],
            })
        }
        _ => Err(miette::miette!(
            help = "Provide both --tls-cert and --tls-key, or omit both to auto-generate.",
            "When using --tls, you must supply both --tls-cert and --tls-key together"
        )),
    }
}

/// Run only the API server
async fn run_serve(bind: &str, data_dir: &str, tls_args: &TlsArgs) -> miette::Result<()> {
    info!("Starting reddwarf API server");

    let state = create_app_state(data_dir)?;

    bootstrap_default_namespace(&state).await?;

    let tls_mode = tls_mode_from_args(tls_args, data_dir)?;

    let config = ApiConfig {
        listen_addr: bind
            .parse()
            .map_err(|e| miette::miette!("Invalid bind address '{}': {}", bind, e))?,
        tls_mode,
    };

    let token = CancellationToken::new();
    let server = ApiServer::new(config, state);
    let server_token = token.clone();

    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.run(server_token).await {
            error!("API server error: {}", e);
        }
    });

    let sig = shutdown_signal().await;
    info!("Received {}, shutting down gracefully...", sig);
    token.cancel();

    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server_handle).await;
    info!("Shutdown complete");

    Ok(())
}

/// Run the full agent: API server + scheduler + pod controller + node agent
#[allow(clippy::too_many_arguments)]
async fn run_agent(
    node_name: &str,
    bind: &str,
    data_dir: &str,
    storage_pool: &str,
    zones_dataset: Option<&str>,
    images_dataset: Option<&str>,
    volumes_dataset: Option<&str>,
    zonepath_prefix: Option<&str>,
    pod_cidr: &str,
    etherstub_name: &str,
    system_reserved_cpu_millicores: i64,
    system_reserved_memory_bytes: i64,
    max_pods: u32,
    tls_args: &TlsArgs,
) -> miette::Result<()> {
    info!("Starting reddwarf agent for node '{}'", node_name);

    let state = create_app_state(data_dir)?;

    bootstrap_default_namespace(&state).await?;

    let listen_addr: std::net::SocketAddr = bind
        .parse()
        .map_err(|e| miette::miette!("Invalid bind address '{}': {}", bind, e))?;

    // Build storage pool configuration
    let pool_config = StoragePoolConfig::from_pool(storage_pool).with_overrides(
        zones_dataset,
        images_dataset,
        volumes_dataset,
    );

    // Derive zonepath prefix from pool or use explicit override
    let zonepath_prefix = zonepath_prefix
        .unwrap_or_else(|| Box::leak(format!("/{}", pool_config.zones_dataset).into_boxed_str()));

    // Create and initialize storage engine
    let storage_engine: Arc<dyn StorageEngine> = create_storage_engine(pool_config);
    storage_engine
        .initialize()
        .await
        .map_err(|e| miette::miette!("Failed to initialize storage: {}", e))?;

    // Build TLS mode
    let tls_mode = tls_mode_from_args(tls_args, data_dir)?;
    let tls_enabled = !matches!(tls_mode, TlsMode::Disabled);

    // Determine the API URL for internal components
    let scheme = if tls_enabled { "https" } else { "http" };
    let api_url = format!("{scheme}://127.0.0.1:{}", listen_addr.port());

    let token = CancellationToken::new();

    // 1. Build API server and resolve TLS material *before* spawning
    let api_config = ApiConfig {
        listen_addr,
        tls_mode,
    };
    let api_server = ApiServer::new(api_config, state.clone());

    let tls_material = api_server.resolve_tls_material()?;
    let ca_pem = tls_material.as_ref().and_then(|m| m.ca_pem.clone());

    let api_token = token.clone();
    let api_handle = tokio::spawn(async move {
        if let Err(e) = api_server.run(api_token).await {
            error!("API server error: {}", e);
        }
    });

    // Give the API server a moment to start listening
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // 2. Spawn scheduler
    let scheduler = Scheduler::new(
        state.storage.clone(),
        state.version_store.clone(),
        state.event_tx.clone(),
        SchedulerConfig::default(),
    );
    let scheduler_token = token.clone();
    let scheduler_handle = tokio::spawn(async move {
        if let Err(e) = scheduler.run(scheduler_token).await {
            error!("Scheduler error: {}", e);
        }
    });

    // 3. Create runtime with injected storage engine
    let runtime: Arc<dyn reddwarf_runtime::ZoneRuntime> = create_runtime(storage_engine);

    // 4. Create IPAM for per-pod IP allocation
    let ipam = Ipam::new(state.storage.clone(), pod_cidr).map_err(|e| {
        miette::miette!("Failed to initialize IPAM with CIDR '{}': {}", pod_cidr, e)
    })?;

    // 5. Spawn pod controller
    let api_client = Arc::new(ApiClient::with_ca_cert(&api_url, ca_pem.as_deref()));
    let controller_config = PodControllerConfig {
        node_name: node_name.to_string(),
        api_url: api_url.clone(),
        zonepath_prefix: zonepath_prefix.to_string(),
        default_brand: ZoneBrand::Reddwarf,
        etherstub_name: etherstub_name.to_string(),
        pod_cidr: pod_cidr.to_string(),
        reconcile_interval: std::time::Duration::from_secs(30),
    };

    let controller = PodController::new(
        runtime,
        api_client.clone(),
        state.event_tx.clone(),
        controller_config,
        ipam,
    );
    let controller_token = token.clone();
    let controller_handle = tokio::spawn(async move {
        if let Err(e) = controller.run(controller_token).await {
            error!("Pod controller error: {}", e);
        }
    });

    // 6. Spawn node agent
    let mut node_agent_config = NodeAgentConfig::new(node_name.to_string(), api_url);
    node_agent_config.system_reserved_cpu_millicores = system_reserved_cpu_millicores;
    node_agent_config.system_reserved_memory_bytes = system_reserved_memory_bytes;
    node_agent_config.max_pods = max_pods;
    let node_agent = NodeAgent::new(api_client.clone(), node_agent_config);
    let agent_token = token.clone();
    let node_agent_handle = tokio::spawn(async move {
        if let Err(e) = node_agent.run(agent_token).await {
            error!("Node agent error: {}", e);
        }
    });

    // 7. Spawn node health checker
    let health_checker = NodeHealthChecker::new(api_client, NodeHealthCheckerConfig::default());
    let health_token = token.clone();
    let health_handle = tokio::spawn(async move {
        if let Err(e) = health_checker.run(health_token).await {
            error!("Node health checker error: {}", e);
        }
    });

    info!(
        "All components started. API server on {}, node name: {}, pod CIDR: {}",
        bind, node_name, pod_cidr
    );

    // Wait for shutdown signal (SIGINT or SIGTERM)
    let sig = shutdown_signal().await;
    info!("Received {}, shutting down gracefully...", sig);
    token.cancel();

    // Wait for all tasks to finish with a timeout
    let shutdown_timeout = std::time::Duration::from_secs(5);
    let _ = tokio::time::timeout(shutdown_timeout, async {
        let _ = tokio::join!(
            api_handle,
            scheduler_handle,
            controller_handle,
            node_agent_handle,
            health_handle,
        );
    })
    .await;

    info!("Shutdown complete");

    Ok(())
}

/// Bootstrap the "default" namespace if it doesn't already exist
async fn bootstrap_default_namespace(state: &AppState) -> miette::Result<()> {
    use reddwarf_apiserver::handlers::common::create_resource;

    let mut ns = Namespace::default();
    ns.metadata.name = Some("default".to_string());

    match create_resource(state, ns).await {
        Ok(_) => info!("Created default namespace"),
        Err(ApiError::AlreadyExists(_)) => {
            // Already exists — fine
        }
        Err(e) => {
            return Err(miette::miette!(
                "Failed to bootstrap default namespace: {:?}",
                e
            ))
        }
    }
    Ok(())
}

/// Create the shared application state
fn create_app_state(data_dir: &str) -> miette::Result<Arc<AppState>> {
    let storage = Arc::new(
        RedbBackend::new(std::path::Path::new(data_dir))
            .map_err(|e| miette::miette!("Failed to open storage at '{}': {}", data_dir, e))?,
    );

    let version_store = Arc::new(
        VersionStore::new(storage.clone())
            .map_err(|e| miette::miette!("Failed to create version store: {}", e))?,
    );

    Ok(Arc::new(AppState::new(storage, version_store)))
}

/// Create the appropriate storage engine for this platform
fn create_storage_engine(config: StoragePoolConfig) -> Arc<dyn StorageEngine> {
    #[cfg(target_os = "illumos")]
    {
        info!("Using ZfsStorageEngine (native ZFS support)");
        Arc::new(reddwarf_runtime::ZfsStorageEngine::new(config))
    }
    #[cfg(not(target_os = "illumos"))]
    {
        info!("Using MockStorageEngine (in-memory storage for development)");
        Arc::new(MockStorageEngine::new(config))
    }
}

/// Create the appropriate zone runtime for this platform
fn create_runtime(storage: Arc<dyn StorageEngine>) -> Arc<dyn reddwarf_runtime::ZoneRuntime> {
    #[cfg(target_os = "illumos")]
    {
        info!("Using IllumosRuntime (native zone support)");
        Arc::new(reddwarf_runtime::IllumosRuntime::new(storage))
    }
    #[cfg(not(target_os = "illumos"))]
    {
        info!("Using MockRuntime (illumos zone emulation for development)");
        Arc::new(MockRuntime::new(storage))
    }
}
