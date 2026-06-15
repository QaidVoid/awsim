use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use awsim_core::AwsError;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use crate::types::text_to_field;

/// Metadata for one column of a query result.
pub struct ColumnMeta {
    pub name: String,
    pub type_oid: i64,
    pub type_name: String,
}

/// The outcome of running a single statement.
pub enum ExecResult {
    /// A data-modifying statement, with the number of rows it affected.
    Update { rows_affected: u64 },
    /// A row-returning statement, with its columns and mapped records.
    Query {
        columns: Vec<ColumnMeta>,
        records: Vec<Vec<serde_json::Value>>,
    },
}

/// A running PostgreSQL container backing one cluster.
struct Container {
    runtime: String,
    container_id: String,
    host_port: u16,
    client: Arc<Client>,
}

impl Drop for Container {
    fn drop(&mut self) {
        // Best-effort teardown. The container is also started with
        // `--rm`, so a stop removes it; `rm -f` covers both.
        let _ = std::process::Command::new(&self.runtime)
            .args(["rm", "-f", &self.container_id])
            .output();
    }
}

/// An open transaction holding its own dedicated connection.
struct Transaction {
    client: Arc<Client>,
}

/// Backs the RDS Data API with a real PostgreSQL per cluster, started on
/// demand in a Docker container. Statements run against the cluster's
/// container; transactions each hold a dedicated connection.
pub struct PgEngine {
    /// Container runtime executable, `docker` by default. Podman is
    /// drop-in compatible for the commands used here, so setting this to
    /// `podman` works without further changes.
    runtime: String,
    image: String,
    db_name: String,
    /// Host AWSim connects to in order to reach the published container
    /// port. `127.0.0.1` when AWSim runs on the Docker host. When AWSim
    /// itself runs in a container against the host's Docker socket
    /// (Docker-out-of-Docker), set `AWSIM_RDS_DATA_PG_HOST` to a host the
    /// sibling container is reachable through (for example
    /// `host.docker.internal`).
    host: String,
    containers: Mutex<HashMap<String, Arc<Container>>>,
    transactions: Mutex<HashMap<String, Transaction>>,
}

impl PgEngine {
    pub fn new(runtime: String, image: String, host: String) -> Self {
        Self {
            runtime,
            image,
            db_name: "awsim".to_string(),
            host,
            containers: Mutex::new(HashMap::new()),
            transactions: Mutex::new(HashMap::new()),
        }
    }

    /// Run a statement against a cluster, optionally inside an open
    /// transaction.
    pub async fn execute(
        &self,
        resource_arn: &str,
        transaction_id: Option<&str>,
        sql: &str,
    ) -> Result<ExecResult, AwsError> {
        let client = match transaction_id {
            Some(id) => {
                let txns = self.transactions.lock().await;
                txns.get(id)
                    .map(|t| t.client.clone())
                    .ok_or_else(|| bad_request(format!("Transaction `{id}` is not open.")))?
            }
            None => self.ensure_container(resource_arn).await?.client.clone(),
        };
        run_sql(&client, sql).await
    }

    /// Open a transaction on a cluster and return its identifier.
    pub async fn begin(&self, resource_arn: &str) -> Result<String, AwsError> {
        let container = self.ensure_container(resource_arn).await?;
        let client = connect(&self.host, container.host_port, &self.db_name)
            .await
            .map_err(|e| {
                AwsError::internal(format!("failed to open transaction connection: {e}"))
            })?;
        client.batch_execute("BEGIN").await.map_err(pg_error)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.transactions.lock().await.insert(
            id.clone(),
            Transaction {
                client: Arc::new(client),
            },
        );
        Ok(id)
    }

    /// Commit and close an open transaction.
    pub async fn commit(&self, transaction_id: &str) -> Result<(), AwsError> {
        let txn = self
            .transactions
            .lock()
            .await
            .remove(transaction_id)
            .ok_or_else(|| bad_request(format!("Transaction `{transaction_id}` is not open.")))?;
        txn.client.batch_execute("COMMIT").await.map_err(pg_error)?;
        Ok(())
    }

    /// Roll back and close an open transaction.
    pub async fn rollback(&self, transaction_id: &str) -> Result<(), AwsError> {
        let txn = self
            .transactions
            .lock()
            .await
            .remove(transaction_id)
            .ok_or_else(|| bad_request(format!("Transaction `{transaction_id}` is not open.")))?;
        txn.client
            .batch_execute("ROLLBACK")
            .await
            .map_err(pg_error)?;
        Ok(())
    }

    async fn ensure_container(&self, resource_arn: &str) -> Result<Arc<Container>, AwsError> {
        let mut map = self.containers.lock().await;
        if let Some(c) = map.get(resource_arn) {
            return Ok(c.clone());
        }
        let container = self.launch_container().await?;
        let arc = Arc::new(container);
        map.insert(resource_arn.to_string(), arc.clone());
        Ok(arc)
    }

    async fn launch_container(&self) -> Result<Container, AwsError> {
        let port = free_port()
            .map_err(|e| AwsError::internal(format!("failed to allocate a host port: {e}")))?;
        // Bind to loopback only for the on-host case. When AWSim reaches
        // the container through a non-loopback host (Docker-out-of-Docker),
        // publish on all interfaces so the sibling container is reachable.
        let publish_bind = if self.host == "127.0.0.1" {
            format!("127.0.0.1:{port}:5432")
        } else {
            format!("0.0.0.0:{port}:5432")
        };
        let output = Command::new(&self.runtime)
            .args([
                "run",
                "-d",
                "--rm",
                "-e",
                "POSTGRES_PASSWORD=awsim",
                "-e",
                &format!("POSTGRES_DB={}", self.db_name),
                "-p",
                &publish_bind,
                &self.image,
            ])
            .output()
            .await
            .map_err(|e| {
                AwsError::internal(format!(
                    "failed to start a PostgreSQL container with `{}` (is the container runtime installed and running?): {e}",
                    self.runtime
                ))
            })?;
        if !output.status.success() {
            return Err(AwsError::internal(format!(
                "`{} run` failed: {}",
                self.runtime,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        match wait_for_ready(&self.host, port, &self.db_name).await {
            Ok(client) => Ok(Container {
                runtime: self.runtime.clone(),
                container_id,
                host_port: port,
                client: Arc::new(client),
            }),
            Err(e) => {
                let _ = std::process::Command::new(&self.runtime)
                    .args(["rm", "-f", &container_id])
                    .output();
                Err(AwsError::internal(format!(
                    "PostgreSQL container did not become ready: {e}"
                )))
            }
        }
    }
}

async fn run_sql(client: &Client, sql: &str) -> Result<ExecResult, AwsError> {
    let stmt = client.prepare(sql).await.map_err(pg_error)?;
    if stmt.columns().is_empty() {
        let rows_affected = client.execute(&stmt, &[]).await.map_err(pg_error)?;
        return Ok(ExecResult::Update { rows_affected });
    }

    let columns: Vec<ColumnMeta> = stmt
        .columns()
        .iter()
        .map(|c| ColumnMeta {
            name: c.name().to_string(),
            type_oid: c.type_().oid() as i64,
            type_name: c.type_().name().to_string(),
        })
        .collect();

    let messages = client.simple_query(sql).await.map_err(pg_error)?;
    let mut records = Vec::new();
    for message in messages {
        if let SimpleQueryMessage::Row(row) = message {
            let fields = columns
                .iter()
                .enumerate()
                .map(|(idx, col)| text_to_field(&col.type_name, row.get(idx)))
                .collect();
            records.push(fields);
        }
    }
    Ok(ExecResult::Query { columns, records })
}

async fn wait_for_ready(host: &str, port: u16, db: &str) -> Result<Client, String> {
    let mut last_err = String::new();
    for _ in 0..120 {
        match connect(host, port, db).await {
            Ok(client) => return Ok(client),
            Err(e) => last_err = e.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(last_err)
}

async fn connect(host: &str, port: u16, db: &str) -> Result<Client, tokio_postgres::Error> {
    let conn_str = format!(
        "host={host} port={port} user=postgres password=awsim dbname={db} connect_timeout=2"
    );
    let (client, connection) = tokio_postgres::connect(&conn_str, NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn pg_error(err: tokio_postgres::Error) -> AwsError {
    AwsError::bad_request("BadRequestException", err.to_string())
}

fn bad_request(message: impl Into<String>) -> AwsError {
    AwsError::bad_request("BadRequestException", message)
}
