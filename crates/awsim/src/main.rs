use anyhow::Result;
use clap::Parser;
use tracing::info;

use awsim_core::AppState;

#[derive(Parser)]
#[command(name = "awsim", about = "AWSim — fully offline, free AWS development environment")]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "4566", env = "AWSIM_PORT")]
    port: u16,

    /// Default AWS region
    #[arg(short, long, default_value = "us-east-1", env = "AWSIM_REGION")]
    region: String,

    /// Default AWS account ID
    #[arg(long, default_value = "000000000000", env = "AWSIM_ACCOUNT_ID")]
    account_id: String,

    /// Data directory for persistence (omit for in-memory only)
    #[arg(long, env = "AWSIM_DATA_DIR")]
    data_dir: Option<String>,

    /// Log level
    #[arg(short = 'v', long, default_value = "info", env = "AWSIM_LOG_LEVEL")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .init();

    let mut state = AppState::new(cli.region.clone(), cli.account_id.clone());

    // Register all services
    register_services(&mut state);

    let service_count = state.services.len();

    let app = axum::Router::new()
        .route("/_awsim/health", axum::routing::get(health))
        .fallback(awsim_core::gateway::handle_request)
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], cli.port));
    info!(
        port = cli.port,
        region = %cli.region,
        account_id = %cli.account_id,
        services = service_count,
        "AWSim started"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn register_services(state: &mut AppState) {
    use std::sync::Arc;

    let sts = Arc::new(awsim_sts::StsService::new());
    state.register(sts, vec![]);
}

async fn health() -> &'static str {
    r#"{"status":"ok","service":"awsim"}"#
}
