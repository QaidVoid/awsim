//! SQLite-backed log-event storage. The previous in-memory `Vec<LogEvent>`
//! per stream had no retention enforcement and triggered an O(n log n)
//! sort on every `PutLogEvents` — fine for a smoke test, deadly under
//! any sustained logging workload.
//!
//! Layout: a single `log_events` table partitioned by (account, region,
//! log_group, log_stream). All queries use the composite index, so reads
//! stay cheap even at hundreds of thousands of events. Retention is
//! enforced by a single `DELETE WHERE ts < ?` per group.

use std::path::PathBuf;
use std::sync::Arc;

use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, params};

use awsim_core::AwsError;

const POOL_MAX: u32 = 4;
const POOL_MIN_IDLE: u32 = 1;
const CACHE_SIZE_KIB: i64 = -2 * 1024;
const MMAP_SIZE_BYTES: i64 = 16 * 1024 * 1024;
const WAL_AUTOCHECKPOINT_PAGES: i64 = 256;

type Pool = r2d2::Pool<SqliteConnectionManager>;
type Conn = PooledConnection<SqliteConnectionManager>;

/// SQLite-backed store for CloudWatch Logs events. Cheap to clone —
/// internals are an Arc'd r2d2 pool.
#[derive(Clone, Debug)]
pub struct SqliteStore {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    db_path: PathBuf,
    #[allow(dead_code)] // pool's Debug is what we care about
    pool: Pool,
}

#[derive(Debug, Clone)]
pub struct LogEventRow {
    pub timestamp: u64,
    pub message: String,
    pub ingestion_time: u64,
}

impl SqliteStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AwsError> {
        let db_path = path.into();
        let manager = SqliteConnectionManager::file(&db_path).with_init(apply_pragmas);
        let pool = r2d2::Pool::builder()
            .max_size(POOL_MAX)
            .min_idle(Some(POOL_MIN_IDLE))
            .build(manager)
            .map_err(|e| AwsError::internal(format!("CWL pool init failed: {e}")))?;
        // Run migrations (just one for now).
        {
            let conn = pool
                .get()
                .map_err(|e| AwsError::internal(format!("CWL pool acquire failed: {e}")))?;
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
            .map_err(|e| AwsError::internal(format!("CWL pool acquire failed: {e}")))
    }

    /// Bulk-insert log events. Returns the number of events written.
    pub fn put_events(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
        log_stream: &str,
        events: &[LogEventRow],
    ) -> Result<usize, AwsError> {
        if events.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(sqlite_err)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO log_events
                     (account, region, log_group, log_stream, ts, ingestion_ts, message)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(sqlite_err)?;
            for e in events {
                stmt.execute(params![
                    account,
                    region,
                    log_group,
                    log_stream,
                    e.timestamp as i64,
                    e.ingestion_time as i64,
                    &e.message,
                ])
                .map_err(sqlite_err)?;
            }
        }
        tx.commit().map_err(sqlite_err)?;
        Ok(events.len())
    }

    /// Range-query log events for a single stream. `start` / `end` in ms.
    /// Pagination via (timestamp, rowid) tuple → caller passes the
    /// last-seen rowid as `after_rowid` to resume.
    #[allow(clippy::too_many_arguments)]
    pub fn get_events(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
        log_stream: &str,
        start: Option<u64>,
        end: Option<u64>,
        offset: usize,
        limit: usize,
        ascending: bool,
    ) -> Result<Vec<LogEventRow>, AwsError> {
        let conn = self.conn()?;
        let order = if ascending { "ASC" } else { "DESC" };
        let sql = format!(
            "SELECT ts, ingestion_ts, message FROM log_events
             WHERE account = ?1 AND region = ?2 AND log_group = ?3 AND log_stream = ?4
               AND (?5 IS NULL OR ts >= ?5)
               AND (?6 IS NULL OR ts <= ?6)
             ORDER BY ts {order}, rowid {order}
             LIMIT ?7 OFFSET ?8"
        );
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let start_param = start.map(|v| v as i64);
        let end_param = end.map(|v| v as i64);
        let rows = stmt
            .query_map(
                params![
                    account,
                    region,
                    log_group,
                    log_stream,
                    start_param,
                    end_param,
                    limit as i64,
                    offset as i64,
                ],
                |row| {
                    Ok(LogEventRow {
                        timestamp: row.get::<_, i64>(0)? as u64,
                        ingestion_time: row.get::<_, i64>(1)? as u64,
                        message: row.get::<_, String>(2)?,
                    })
                },
            )
            .map_err(sqlite_err)?;
        let out: Result<Vec<_>, _> = rows.collect();
        out.map_err(sqlite_err)
    }

    /// Total event count for a single stream — used to compute
    /// pagination tokens that mirror the legacy index-based ones.
    pub fn count_events(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
        log_stream: &str,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM log_events
                 WHERE account = ?1 AND region = ?2 AND log_group = ?3 AND log_stream = ?4
                   AND (?5 IS NULL OR ts >= ?5)
                   AND (?6 IS NULL OR ts <= ?6)",
                params![
                    account,
                    region,
                    log_group,
                    log_stream,
                    start.map(|v| v as i64),
                    end.map(|v| v as i64),
                ],
                |r| r.get(0),
            )
            .map_err(sqlite_err)?;
        Ok(count as usize)
    }

    /// Filter events across one or more streams by substring match
    /// on `message`, returned in `(stream, ts)` order.
    #[allow(clippy::too_many_arguments)]
    pub fn filter_events(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
        stream_filter: Option<&[String]>,
        substring: Option<&str>,
        start: Option<u64>,
        end: Option<u64>,
        limit: usize,
    ) -> Result<Vec<(String, LogEventRow)>, AwsError> {
        let conn = self.conn()?;
        let mut sql = String::from(
            "SELECT log_stream, ts, ingestion_ts, message FROM log_events
             WHERE account = ?1 AND region = ?2 AND log_group = ?3
               AND (?4 IS NULL OR ts >= ?4)
               AND (?5 IS NULL OR ts <= ?5)",
        );
        if let Some(s) = substring
            && !s.is_empty()
        {
            sql.push_str(&format!(
                " AND message LIKE '%' || {} || '%'",
                escape_for_like(s)
            ));
        }
        if let Some(streams) = stream_filter
            && !streams.is_empty()
        {
            sql.push_str(" AND log_stream IN (");
            for (i, s) in streams.iter().enumerate() {
                if i > 0 {
                    sql.push(',');
                }
                sql.push('\'');
                sql.push_str(&s.replace('\'', "''"));
                sql.push('\'');
            }
            sql.push(')');
        }
        sql.push_str(" ORDER BY ts ASC, rowid ASC LIMIT ?6");
        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let start_param = start.map(|v| v as i64);
        let end_param = end.map(|v| v as i64);
        let rows = stmt
            .query_map(
                params![
                    account,
                    region,
                    log_group,
                    start_param,
                    end_param,
                    limit as i64,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        LogEventRow {
                            timestamp: row.get::<_, i64>(1)? as u64,
                            ingestion_time: row.get::<_, i64>(2)? as u64,
                            message: row.get::<_, String>(3)?,
                        },
                    ))
                },
            )
            .map_err(sqlite_err)?;
        let out: Result<Vec<_>, _> = rows.collect();
        out.map_err(sqlite_err)
    }

    /// First / last event timestamps for a stream — used to populate
    /// the `firstEventTimestamp` / `lastEventTimestamp` fields on
    /// DescribeLogStreams. Returns `(None, None)` when the stream is
    /// empty.
    pub fn stream_bounds(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
        log_stream: &str,
    ) -> Result<(Option<u64>, Option<u64>), AwsError> {
        let conn = self.conn()?;
        let row: Option<(Option<i64>, Option<i64>)> = conn
            .query_row(
                "SELECT MIN(ts), MAX(ts) FROM log_events
                 WHERE account = ?1 AND region = ?2 AND log_group = ?3 AND log_stream = ?4",
                params![account, region, log_group, log_stream],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()
            .map_err(sqlite_err)?;
        Ok(row
            .map(|(a, b)| (a.map(|v| v as u64), b.map(|v| v as u64)))
            .unwrap_or((None, None)))
    }

    /// Delete events older than `cutoff_ts` (ms). Used by the
    /// retention sweeper. Returns the number of rows deleted.
    pub fn trim_older_than(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
        cutoff_ts: u64,
    ) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM log_events
                 WHERE account = ?1 AND region = ?2 AND log_group = ?3 AND ts < ?4",
                params![account, region, log_group, cutoff_ts as i64],
            )
            .map_err(sqlite_err)?;
        Ok(n)
    }

    /// Delete every event for a stream — used when DeleteLogStream
    /// fires. Cheap: indexed lookup + bulk delete.
    pub fn delete_stream(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
        log_stream: &str,
    ) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM log_events
                 WHERE account = ?1 AND region = ?2 AND log_group = ?3 AND log_stream = ?4",
                params![account, region, log_group, log_stream],
            )
            .map_err(sqlite_err)?;
        Ok(n)
    }

    /// Delete every event for a log group — used by DeleteLogGroup.
    pub fn delete_group(
        &self,
        account: &str,
        region: &str,
        log_group: &str,
    ) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM log_events
                 WHERE account = ?1 AND region = ?2 AND log_group = ?3",
                params![account, region, log_group],
            )
            .map_err(sqlite_err)?;
        Ok(n)
    }

    /// Total row count across all groups + streams. Used by the
    /// `/_awsim/storage` endpoint to surface in-memory growth.
    pub fn total_rows(&self) -> Result<u64, AwsError> {
        let conn = self.conn()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM log_events", [], |r| r.get(0))
            .map_err(sqlite_err)?;
        Ok(n as u64)
    }
}

fn init_schema(conn: &Connection) -> Result<(), AwsError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS log_events (
             account TEXT NOT NULL,
             region TEXT NOT NULL,
             log_group TEXT NOT NULL,
             log_stream TEXT NOT NULL,
             ts INTEGER NOT NULL,
             ingestion_ts INTEGER NOT NULL,
             message TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS log_events_lookup
             ON log_events (account, region, log_group, log_stream, ts);
         CREATE INDEX IF NOT EXISTS log_events_group_ts
             ON log_events (account, region, log_group, ts);",
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

/// Naïve quoted literal for substring matching. Wraps the user
/// string in single quotes and escapes embedded quotes. We can't
/// bind the LIKE pattern as a parameter directly because we want
/// to control the wildcards.
fn escape_for_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn sqlite_err(e: rusqlite::Error) -> AwsError {
    AwsError::internal(format!("CloudWatch Logs sqlite error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SqliteStore {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-cwl-test-{id}.db"));
        SqliteStore::open(path).unwrap()
    }

    fn ev(ts: u64, msg: &str) -> LogEventRow {
        LogEventRow {
            timestamp: ts,
            message: msg.to_string(),
            ingestion_time: ts + 1,
        }
    }

    #[test]
    fn put_then_get_returns_in_ts_order() {
        let s = store();
        s.put_events("a", "r", "g", "stm", &[ev(3, "c"), ev(1, "a"), ev(2, "b")])
            .unwrap();
        let rows = s
            .get_events("a", "r", "g", "stm", None, None, 0, 100, true)
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].timestamp, 1);
        assert_eq!(rows[2].timestamp, 3);
    }

    #[test]
    fn time_range_filter() {
        let s = store();
        s.put_events("a", "r", "g", "stm", &[ev(1, "a"), ev(5, "b"), ev(10, "c")])
            .unwrap();
        let rows = s
            .get_events("a", "r", "g", "stm", Some(2), Some(7), 0, 100, true)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].timestamp, 5);
    }

    #[test]
    fn filter_substring_across_streams() {
        let s = store();
        s.put_events("a", "r", "g", "s1", &[ev(1, "hello world")])
            .unwrap();
        s.put_events(
            "a",
            "r",
            "g",
            "s2",
            &[ev(2, "no match"), ev(3, "world cup")],
        )
        .unwrap();
        let rows = s
            .filter_events("a", "r", "g", None, Some("world"), None, None, 100)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1.timestamp, 1);
        assert_eq!(rows[1].1.timestamp, 3);
    }

    #[test]
    fn trim_older_than_drops_events() {
        let s = store();
        s.put_events("a", "r", "g", "stm", &[ev(1, "a"), ev(5, "b"), ev(10, "c")])
            .unwrap();
        let removed = s.trim_older_than("a", "r", "g", 5).unwrap();
        assert_eq!(removed, 1);
        let remaining = s.count_events("a", "r", "g", "stm", None, None).unwrap();
        assert_eq!(remaining, 2);
    }

    #[test]
    fn stream_bounds_returns_min_max() {
        let s = store();
        s.put_events("a", "r", "g", "stm", &[ev(5, "x"), ev(10, "y"), ev(2, "z")])
            .unwrap();
        let (min, max) = s.stream_bounds("a", "r", "g", "stm").unwrap();
        assert_eq!(min, Some(2));
        assert_eq!(max, Some(10));
    }

    #[test]
    fn delete_stream_removes_only_that_stream() {
        let s = store();
        s.put_events("a", "r", "g", "s1", &[ev(1, "a")]).unwrap();
        s.put_events("a", "r", "g", "s2", &[ev(1, "b")]).unwrap();
        s.delete_stream("a", "r", "g", "s1").unwrap();
        assert_eq!(s.count_events("a", "r", "g", "s1", None, None).unwrap(), 0);
        assert_eq!(s.count_events("a", "r", "g", "s2", None, None).unwrap(), 1);
    }
}
