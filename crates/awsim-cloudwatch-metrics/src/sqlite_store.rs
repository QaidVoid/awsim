//! SQLite-backed storage for CloudWatch Metrics datapoints. The
//! previous in-memory `DashMap<String, Vec<MetricDatum>>` had no
//! retention enforcement — every PutMetricData appended forever. Now
//! datapoints live in a SQLite table indexed by (account, region,
//! namespace, metric_name, ts_ms), and a periodic sweeper trims by
//! retention (default 15 days, mirroring AWS's retention for high-
//! resolution datapoints).
//!
//! Alarms and dashboards stay in DashMap on `CloudWatchState` — they
//! are small (one entry per alarm/dashboard) and read on every alarm
//! evaluation, so the in-memory map is fine.

use std::path::PathBuf;
use std::sync::Arc;

use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, params};
use serde_json::Value;

use awsim_core::AwsError;

const POOL_MAX: u32 = 4;
const POOL_MIN_IDLE: u32 = 1;
const CACHE_SIZE_KIB: i64 = -2 * 1024;
const MMAP_SIZE_BYTES: i64 = 16 * 1024 * 1024;
const WAL_AUTOCHECKPOINT_PAGES: i64 = 256;

type Pool = r2d2::Pool<SqliteConnectionManager>;
type Conn = PooledConnection<SqliteConnectionManager>;

#[derive(Clone, Debug)]
pub struct SqliteStore {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    db_path: PathBuf,
    #[allow(dead_code)]
    pool: Pool,
}

#[derive(Debug, Clone)]
pub struct MetricDatumRow {
    pub namespace: String,
    pub metric_name: String,
    pub value: f64,
    pub unit: String,
    /// RFC3339 timestamp string (echoed back to clients verbatim).
    pub timestamp: String,
    /// Parsed timestamp in epoch-ms — used for indexing + retention.
    pub ts_ms: i64,
    /// Wire-format dimensions: `[{"Name":..,"Value":..}, ...]`.
    pub dimensions_json: Value,
}

impl SqliteStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AwsError> {
        let db_path = path.into();
        let manager = SqliteConnectionManager::file(&db_path).with_init(apply_pragmas);
        let pool = r2d2::Pool::builder()
            .max_size(POOL_MAX)
            .min_idle(Some(POOL_MIN_IDLE))
            .build(manager)
            .map_err(|e| AwsError::internal(format!("CWM pool init failed: {e}")))?;
        {
            let conn = pool
                .get()
                .map_err(|e| AwsError::internal(format!("CWM pool acquire failed: {e}")))?;
            init_schema(&conn)?;
        }
        Ok(Self {
            inner: Arc::new(Inner { db_path, pool }),
        })
    }

    pub fn db_path(&self) -> &std::path::Path {
        &self.inner.db_path
    }

    fn conn(&self) -> Result<Conn, AwsError> {
        self.inner
            .pool
            .get()
            .map_err(|e| AwsError::internal(format!("CWM pool acquire failed: {e}")))
    }

    /// Bulk-insert datapoints. The caller pre-parses the RFC3339
    /// timestamp into `ts_ms` so range scans / retention sweeps stay
    /// cheap.
    pub fn put_datapoints(
        &self,
        account: &str,
        region: &str,
        rows: &[MetricDatumRow],
    ) -> Result<usize, AwsError> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(sqlite_err)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO datapoints
                     (account, region, namespace, metric_name, value, unit,
                      timestamp, ts_ms, dimensions_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )
                .map_err(sqlite_err)?;
            for r in rows {
                stmt.execute(params![
                    account,
                    region,
                    &r.namespace,
                    &r.metric_name,
                    r.value,
                    &r.unit,
                    &r.timestamp,
                    r.ts_ms,
                    serde_json::to_string(&r.dimensions_json).unwrap_or_else(|_| "[]".to_string()),
                ])
                .map_err(sqlite_err)?;
            }
        }
        tx.commit().map_err(sqlite_err)?;
        Ok(rows.len())
    }

    /// Range-query datapoints for a single metric. `start_ms` and
    /// `end_ms` are inclusive bounds on `ts_ms`.
    pub fn get_datapoints(
        &self,
        account: &str,
        region: &str,
        namespace: &str,
        metric_name: &str,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> Result<Vec<MetricDatumRow>, AwsError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT namespace, metric_name, value, unit, timestamp, ts_ms, dimensions_json
                 FROM datapoints
                 WHERE account = ?1 AND region = ?2 AND namespace = ?3 AND metric_name = ?4
                   AND (?5 IS NULL OR ts_ms >= ?5)
                   AND (?6 IS NULL OR ts_ms <= ?6)
                 ORDER BY ts_ms ASC, rowid ASC",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map(
                params![account, region, namespace, metric_name, start_ms, end_ms],
                |row| {
                    let dims_str: String = row.get(6)?;
                    Ok(MetricDatumRow {
                        namespace: row.get(0)?,
                        metric_name: row.get(1)?,
                        value: row.get(2)?,
                        unit: row.get(3)?,
                        timestamp: row.get(4)?,
                        ts_ms: row.get(5)?,
                        dimensions_json: serde_json::from_str(&dims_str)
                            .unwrap_or_else(|_| Value::Array(Vec::new())),
                    })
                },
            )
            .map_err(sqlite_err)?;
        let out: Result<Vec<_>, _> = rows.collect();
        out.map_err(sqlite_err)
    }

    /// Distinct (namespace, metric_name, dimensions) tuples for
    /// `ListMetrics`. Cheaper than scanning everything since
    /// SQLite's index can short-circuit duplicates.
    pub fn list_metrics(
        &self,
        account: &str,
        region: &str,
        namespace_filter: Option<&str>,
        metric_name_filter: Option<&str>,
    ) -> Result<Vec<(String, String, Value)>, AwsError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT namespace, metric_name, dimensions_json
                 FROM datapoints
                 WHERE account = ?1 AND region = ?2
                   AND (?3 IS NULL OR namespace = ?3)
                   AND (?4 IS NULL OR metric_name = ?4)",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map(
                params![account, region, namespace_filter, metric_name_filter],
                |row| {
                    let dims_str: String = row.get(2)?;
                    let dims = serde_json::from_str(&dims_str).unwrap_or(Value::Array(Vec::new()));
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, dims))
                },
            )
            .map_err(sqlite_err)?;
        let out: Result<Vec<_>, _> = rows.collect();
        out.map_err(sqlite_err)
    }

    /// Drop datapoints older than `cutoff_ms`. Returns the row count.
    pub fn trim_older_than(
        &self,
        account: &str,
        region: &str,
        cutoff_ms: i64,
    ) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM datapoints
                 WHERE account = ?1 AND region = ?2 AND ts_ms < ?3",
                params![account, region, cutoff_ms],
            )
            .map_err(sqlite_err)?;
        Ok(n)
    }

    pub fn total_rows(&self) -> Result<u64, AwsError> {
        let conn = self.conn()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM datapoints", [], |r| r.get(0))
            .map_err(sqlite_err)?;
        Ok(n as u64)
    }
}

fn init_schema(conn: &Connection) -> Result<(), AwsError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS datapoints (
             account TEXT NOT NULL,
             region TEXT NOT NULL,
             namespace TEXT NOT NULL,
             metric_name TEXT NOT NULL,
             value REAL NOT NULL,
             unit TEXT NOT NULL,
             timestamp TEXT NOT NULL,
             ts_ms INTEGER NOT NULL,
             dimensions_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS datapoints_lookup
             ON datapoints (account, region, namespace, metric_name, ts_ms);
         CREATE INDEX IF NOT EXISTS datapoints_retention
             ON datapoints (account, region, ts_ms);",
    )
    .map_err(sqlite_err)?;
    Ok(())
}

fn apply_pragmas(conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(&format!(
        "PRAGMA temp_store = MEMORY;
         PRAGMA mmap_size  = {MMAP_SIZE_BYTES};
         PRAGMA cache_size = {CACHE_SIZE_KIB};
         PRAGMA wal_autocheckpoint = {WAL_AUTOCHECKPOINT_PAGES};"
    ))?;
    Ok(())
}

fn sqlite_err(e: rusqlite::Error) -> AwsError {
    AwsError::internal(format!("CloudWatch Metrics sqlite error: {e}"))
}

/// Parse an RFC3339 / chrono-style timestamp string into epoch-ms.
/// Falls back to "now" when parsing fails so a malformed input
/// doesn't lose the datapoint entirely.
pub fn parse_timestamp_ms(s: &str) -> i64 {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.timestamp_millis();
    }
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ") {
        return dt.timestamp_millis();
    }
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SqliteStore {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-cwm-test-{id}.db"));
        SqliteStore::open(path).unwrap()
    }

    fn dp(ns: &str, name: &str, val: f64, ts_ms: i64) -> MetricDatumRow {
        MetricDatumRow {
            namespace: ns.to_string(),
            metric_name: name.to_string(),
            value: val,
            unit: "None".to_string(),
            timestamp: format!("ts-{ts_ms}"),
            ts_ms,
            dimensions_json: serde_json::json!([]),
        }
    }

    #[test]
    fn put_then_get_returns_in_ts_order() {
        let s = store();
        s.put_datapoints(
            "a",
            "r",
            &[
                dp("ns", "m", 3.0, 30),
                dp("ns", "m", 1.0, 10),
                dp("ns", "m", 2.0, 20),
            ],
        )
        .unwrap();
        let rows = s.get_datapoints("a", "r", "ns", "m", None, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].ts_ms, 10);
        assert_eq!(rows[2].ts_ms, 30);
    }

    #[test]
    fn time_range_filter() {
        let s = store();
        s.put_datapoints(
            "a",
            "r",
            &[
                dp("ns", "m", 1.0, 10),
                dp("ns", "m", 2.0, 50),
                dp("ns", "m", 3.0, 100),
            ],
        )
        .unwrap();
        let rows = s
            .get_datapoints("a", "r", "ns", "m", Some(20), Some(70))
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, 2.0);
    }

    #[test]
    fn list_metrics_dedupes_by_metric_name() {
        let s = store();
        s.put_datapoints(
            "a",
            "r",
            &[
                dp("ns1", "a", 1.0, 1),
                dp("ns1", "a", 2.0, 2),
                dp("ns1", "b", 3.0, 3),
                dp("ns2", "a", 4.0, 4),
            ],
        )
        .unwrap();
        let mut metrics = s.list_metrics("a", "r", None, None).unwrap();
        metrics.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0].0, "ns1");
        assert_eq!(metrics[0].1, "a");
        assert_eq!(metrics[2].0, "ns2");
    }

    #[test]
    fn trim_older_than_drops_datapoints() {
        let s = store();
        s.put_datapoints(
            "a",
            "r",
            &[
                dp("ns", "m", 1.0, 10),
                dp("ns", "m", 2.0, 20),
                dp("ns", "m", 3.0, 30),
            ],
        )
        .unwrap();
        let removed = s.trim_older_than("a", "r", 25).unwrap();
        assert_eq!(removed, 2);
        let remaining = s.get_datapoints("a", "r", "ns", "m", None, None).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].value, 3.0);
    }

    #[test]
    fn parse_rfc3339_returns_epoch_ms() {
        let ms = parse_timestamp_ms("2026-04-29T10:30:00Z");
        assert!(ms > 1_000_000_000_000);
    }
}
