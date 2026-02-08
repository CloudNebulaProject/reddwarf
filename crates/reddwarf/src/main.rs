use clap::{Parser, Subcommand};
use reddwarf_apiserver::{ApiError, ApiServer, AppState, Config as ApiConfig};
use reddwarf_core::Namespace;
use reddwarf_runtime::{
    ApiClient, EtherstubConfig, MockRuntime, NetworkMode, NodeAgent, NodeAgentConfig,
    PodController, PodControllerConfig, ZoneBrand,
};
use reddwarf_scheduler::scheduler::SchedulerConfig;
use reddwarf_scheduler::Scheduler;
use reddwarf_storage::RedbBackend;
use reddwarf_versioning::VersionStore;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "reddwarf", about = "Reddwarf Kubernetes Control Plane")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        /// Prefix for zone root paths
        #[arg(long, default_value = "/zones")]
        zonepath_prefix: String,
        /// Parent ZFS dataset for zone storage
        #[arg(long, default_value = "rpool/zones")]
        zfs_parent: String,
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
        Commands::Serve { bind, data_dir } => run_serve(&bind, &data_dir).await,
        Commands::Agent {
            node_name,
            bind,
            data_dir,
            zonepath_prefix,
            zfs_parent,
        } => run_agent(&node_name, &bind, &data_dir, &zonepath_prefix, &zfs_parent).await,
    }
}

/// Run only the API server
async fn run_serve(bind: &str, data_dir: &str) -> miette::Result<()> {
    info!("Starting reddwarf API server");

    let state = create_app_state(data_dir)?;

    bootstrap_default_namespace(&state).await?;

    let config = ApiConfig {
        listen_addr: bind
            .parse()
            .map_err(|e| miette::miette!("Invalid bind address '{}': {}", bind, e))?,
    };

    let server = ApiServer::new(config, state);
    server
        .run()
        .await
        .map_err(|e| miette::miette!("API server error: {}", e))?;

    Ok(())
}

/// Run the full agent: API server + scheduler + pod controller + node agent
async fn run_agent(
    node_name: &str,
    bind: &str,
    data_dir: &str,
    zonepath_prefix: &str,
    zfs_parent: &str,
) -> miette::Result<()> {
    info!("Starting reddwarf agent for node '{}'", node_name);

    let state = create_app_state(data_dir)?;

    bootstrap_default_namespace(&state).await?;

    let listen_addr: std::net::SocketAddr = bind
        .parse()
        .map_err(|e| miette::miette!("Invalid bind address '{}': {}", bind, e))?;

    // Determine the API URL for internal components to connect to
    let api_url = format!("http://127.0.0.1:{}", listen_addr.port());

    let token = CancellationToken::new();

    // 1. Spawn API server
    let api_config = ApiConfig { listen_addr };
    let api_server = ApiServer::new(api_config, state.clone());
    let api_token = token.clone();
    let api_handle = tokio::spawn(async move {
        tokio::select! {
            result = api_server.run() => {
                if let Err(e) = result {
                    error!("API server error: {}", e);
                }
            }
            _ = api_token.cancelled() => {
                info!("API server shutting down");
            }
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

    // 3. Create runtime (MockRuntime on non-illumos, IllumosRuntime on illumos)
    let runtime: Arc<dyn reddwarf_runtime::ZoneRuntime> = create_runtime();

    // 4. Spawn pod controller
    let api_client = Arc::new(ApiClient::new(&api_url));
    let controller_config = PodControllerConfig {
        node_name: node_name.to_string(),
        api_url: api_url.clone(),
        zonepath_prefix: zonepath_prefix.to_string(),
        zfs_parent_dataset: zfs_parent.to_string(),
        default_brand: ZoneBrand::Reddwarf,
        network: NetworkMode::Etherstub(EtherstubConfig {
            etherstub_name: "reddwarf0".to_string(),
            vnic_name: "reddwarf_vnic0".to_string(),
            ip_address: "10.88.0.2".to_string(),
            gateway: "10.88.0.1".to_string(),
        }),
    };

    let controller = PodController::new(
        runtime,
        api_client.clone(),
        state.event_tx.clone(),
        controller_config,
    );
    let controller_token = token.clone();
    let controller_handle = tokio::spawn(async move {
        if let Err(e) = controller.run(controller_token).await {
            error!("Pod controller error: {}", e);
        }
    });

    // 5. Spawn node agent
    let node_agent_config = NodeAgentConfig::new(node_name.to_string(), api_url);
    let node_agent = NodeAgent::new(api_client, node_agent_config);
    let agent_token = token.clone();
    let node_agent_handle = tokio::spawn(async move {
        if let Err(e) = node_agent.run(agent_token).await {
            error!("Node agent error: {}", e);
        }
    });

    info!(
        "All components started. API server on {}, node name: {}",
        bind, node_name
    );

    // Wait for shutdown signal
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| miette::miette!("Failed to listen for ctrl-c: {}", e))?;

    info!("Shutting down gracefully...");
    token.cancel();

    // Wait for all tasks to finish with a timeout
    let shutdown_timeout = std::time::Duration::from_secs(5);
    let _ = tokio::time::timeout(shutdown_timeout, async {
        let _ = tokio::join!(
            api_handle,
            scheduler_handle,
            controller_handle,
            node_agent_handle,
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

/// Create the appropriate zone runtime for this platform
fn create_runtime() -> Arc<dyn reddwarf_runtime::ZoneRuntime> {
    #[cfg(target_os = "illumos")]
    {
        info!("Using IllumosRuntime (native zone support)");
        Arc::new(reddwarf_runtime::IllumosRuntime::new())
    }
    #[cfg(not(target_os = "illumos"))]
    {
        info!("Using MockRuntime (illumos zone emulation for development)");
        Arc::new(MockRuntime::new())
    }
}
