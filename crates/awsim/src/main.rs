use anyhow::Result;
use clap::Parser;
use tracing::info;

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

    info!(
        port = cli.port,
        region = %cli.region,
        account_id = %cli.account_id,
        "Starting AWSim"
    );

    let app = axum::Router::new()
        .route("/_awsim/health", axum::routing::get(health))
        .fallback(handle_aws_request);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], cli.port));
    info!("Listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> &'static str {
    r#"{"status":"ok","service":"awsim"}"#
}

async fn handle_aws_request(
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::http::Response<axum::body::Body> {
    tracing::debug!(
        method = %method,
        uri = %uri,
        content_type = ?headers.get("content-type"),
        target = ?headers.get("x-amz-target"),
        "Incoming AWS request"
    );

    // TODO: Protocol detection, service routing, handler dispatch
    let _ = (method, uri, headers, body);

    axum::http::Response::builder()
        .status(501)
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            r#"{"__type":"NotImplemented","message":"AWSim is starting up — no services registered yet"}"#,
        ))
        .unwrap()
}
