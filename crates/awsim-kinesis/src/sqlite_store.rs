//! SQLite-backed storage for Kinesis records. Replaces the
//! per-shard `Vec<KinesisRecord>` that grew without bound and
//! never honoured `retention_hours`.

use std::path::PathBuf;
use std::sync::Arc;

use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, params};

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
pub struct KinesisRecordRow {
    pub seq: i64,
    pub partition_key: String,
    /// Caller-supplied data — already base64 in the wire format.
    pub data: String,
    pub timestamp_millis: i64,
}

impl SqliteStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AwsError> {
        let db_path = path.into();
        let manager = SqliteConnectionManager::file(&db_path).with_init(apply_pragmas);
        let pool = r2d2::Pool::builder()
            .max_size(POOL_MAX)
            .min_idle(Some(POOL_MIN_IDLE))
            .build(manager)
            .map_err(|e| AwsError::internal(format!("Kinesis pool init failed: {e}")))?;
        {
            let conn = pool
                .get()
                .map_err(|e| AwsError::internal(format!("Kinesis pool acquire failed: {e}")))?;
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
            .map_err(|e| AwsError::internal(format!("Kinesis pool acquire failed: {e}")))
    }

    /// Insert a single record under the caller-supplied sequence
    /// number. The caller (PutRecord / PutRecords) holds the
    /// exclusive write lock on the shard so allocation + insert are
    /// race-free at the awsim layer.
    #[allow(clippy::too_many_arguments)]
    pub fn put_record(
        &self,
        account: &str,
        region: &str,
        stream: &str,
        shard: &str,
        seq: i64,
        partition_key: &str,
        data: &str,
        timestamp_millis: i64,
    ) -> Result<(), AwsError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO records
             (account, region, stream, shard, seq, partition_key, data, ts_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                account,
                region,
                stream,
                shard,
                seq,
                partition_key,
                data,
                timestamp_millis
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Batch-insert helper for PutRecords.
    pub fn put_records(
        &self,
        account: &str,
        region: &str,
        stream: &str,
        rows: &[(String, KinesisRecordRow)],
    ) -> Result<(), AwsError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(sqlite_err)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO records
                     (account, region, stream, shard, seq, partition_key, data, ts_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )
                .map_err(sqlite_err)?;
            for (shard, r) in rows {
                stmt.execute(params![
                    account,
                    region,
                    stream,
                    shard,
                    r.seq,
                    &r.partition_key,
                    &r.data,
                    r.timestamp_millis,
                ])
                .map_err(sqlite_err)?;
            }
        }
        tx.commit().map_err(sqlite_err)?;
        Ok(())
    }

    /// Read records from a shard whose sequence numbers are
    /// `> after_seq`, in ascending order, capped at `limit`.
    pub fn read_after(
        &self,
        account: &str,
        region: &str,
        stream: &str,
        shard: &str,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<KinesisRecordRow>, AwsError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT seq, partition_key, data, ts_ms FROM records
                 WHERE account = ?1 AND region = ?2 AND stream = ?3 AND shard = ?4
                   AND seq > ?5
                 ORDER BY seq ASC
                 LIMIT ?6",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map(
                params![account, region, stream, shard, after_seq, limit as i64],
                |row| {
                    Ok(KinesisRecordRow {
                        seq: row.get(0)?,
                        partition_key: row.get(1)?,
                        data: row.get(2)?,
                        timestamp_millis: row.get(3)?,
                    })
                },
            )
            .map_err(sqlite_err)?;
        let out: Result<Vec<_>, _> = rows.collect();
        out.map_err(sqlite_err)
    }

    /// Highest stored sequence number on a shard. Used by `LATEST`
    /// iterators so they only see records that arrive *after* the
    /// iterator was created.
    pub fn max_seq(
        &self,
        account: &str,
        region: &str,
        stream: &str,
        shard: &str,
    ) -> Result<i64, AwsError> {
        let conn = self.conn()?;
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(seq) FROM records
                 WHERE account = ?1 AND region = ?2 AND stream = ?3 AND shard = ?4",
                params![account, region, stream, shard],
                |r| r.get(0),
            )
            .map_err(sqlite_err)?;
        Ok(max.unwrap_or(0))
    }

    /// Drop records whose `ts_ms < cutoff_ms`. Returns the number
    /// of rows removed. Mirrors a stream's `RetentionPeriodHours`.
    pub fn trim_older_than(
        &self,
        account: &str,
        region: &str,
        stream: &str,
        cutoff_ms: i64,
    ) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM records
                 WHERE account = ?1 AND region = ?2 AND stream = ?3 AND ts_ms < ?4",
                params![account, region, stream, cutoff_ms],
            )
            .map_err(sqlite_err)?;
        Ok(n)
    }

    /// Delete every record on a stream — wired into DeleteStream.
    pub fn delete_stream(
        &self,
        account: &str,
        region: &str,
        stream: &str,
    ) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM records
                 WHERE account = ?1 AND region = ?2 AND stream = ?3",
                params![account, region, stream],
            )
            .map_err(sqlite_err)?;
        Ok(n)
    }

    pub fn total_rows(&self) -> Result<u64, AwsError> {
        let conn = self.conn()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM records", [], |r| r.get(0))
            .map_err(sqlite_err)?;
        Ok(n as u64)
    }
}

fn init_schema(conn: &Connection) -> Result<(), AwsError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS records (
             account TEXT NOT NULL,
             region TEXT NOT NULL,
             stream TEXT NOT NULL,
             shard TEXT NOT NULL,
             seq INTEGER NOT NULL,
             partition_key TEXT NOT NULL,
             data TEXT NOT NULL,
             ts_ms INTEGER NOT NULL,
             PRIMARY KEY (account, region, stream, shard, seq)
         );
         CREATE INDEX IF NOT EXISTS records_retention
             ON records (account, region, stream, ts_ms);",
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
    AwsError::internal(format!("Kinesis sqlite error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SqliteStore {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-kinesis-test-{id}.db"));
        SqliteStore::open(path).unwrap()
    }

    #[test]
    fn put_then_read_after() {
        let s = store();
        for seq in 1..=5 {
            s.put_record("a", "r", "stm", "shardId-0", seq, "pk", "data", seq * 10)
                .unwrap();
        }
        let rows = s.read_after("a", "r", "stm", "shardId-0", 0, 10).unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].seq, 1);
        assert_eq!(rows[4].seq, 5);

        let after2 = s.read_after("a", "r", "stm", "shardId-0", 2, 10).unwrap();
        assert_eq!(after2.len(), 3);
        assert_eq!(after2[0].seq, 3);
    }

    #[test]
    fn limit_caps_returned_rows() {
        let s = store();
        for seq in 1..=10 {
            s.put_record("a", "r", "stm", "shardId-0", seq, "pk", "data", seq * 10)
                .unwrap();
        }
        let rows = s.read_after("a", "r", "stm", "shardId-0", 0, 3).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].seq, 3);
    }

    #[test]
    fn max_seq_returns_zero_when_empty() {
        let s = store();
        let m = s.max_seq("a", "r", "stm", "shardId-0").unwrap();
        assert_eq!(m, 0);
    }

    #[test]
    fn trim_drops_old_records() {
        let s = store();
        for seq in 1..=5 {
            s.put_record("a", "r", "stm", "shardId-0", seq, "pk", "d", seq * 100)
                .unwrap();
        }
        let removed = s.trim_older_than("a", "r", "stm", 350).unwrap();
        assert_eq!(removed, 3);
        let rows = s.read_after("a", "r", "stm", "shardId-0", 0, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].seq, 4);
    }

    #[test]
    fn delete_stream_clears_all_shards() {
        let s = store();
        s.put_record("a", "r", "stm", "s0", 1, "pk", "d", 0)
            .unwrap();
        s.put_record("a", "r", "stm", "s1", 2, "pk", "d", 0)
            .unwrap();
        s.delete_stream("a", "r", "stm").unwrap();
        assert_eq!(s.total_rows().unwrap(), 0);
    }
}
