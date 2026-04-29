//! SQLite-backed storage for DynamoDB items + table metadata.
//!
//! Stage 1 of the DynamoDB-to-SQLite refactor: this module ships the
//! foundation (connection management, migrations, raw item CRUD) but
//! isn't wired into the operation handlers yet. Subsequent stages
//! migrate operations one family at a time (item → query/scan →
//! table metadata → streams/transact/partiql).
//!
//! Concurrency model: rusqlite is sync. Every public method here is
//! itself sync; callers cross the async boundary by wrapping calls
//! in `tokio::task::spawn_blocking` at the operation handler layer.
//! Each call takes a fresh `Connection` from the internal pool —
//! WAL mode means readers never block each other.

use std::path::PathBuf;
use std::sync::Arc;

use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;

use awsim_core::AwsError;

mod embedded_migrations {
    refinery::embed_migrations!("migrations");
}

/// Maximum number of GSIs we materialise to dedicated key columns.
/// DynamoDB's hard cap is 20; this covers every realistic use case
/// while keeping the row width modest.
pub const MAX_GSI_SLOTS: usize = 5;

/// Connection pool ceiling. Lazy: only `MIN_IDLE` connections are
/// kept warm, the pool grows on demand and shrinks back. WAL gives
/// unlimited concurrent readers and one writer, so a 4-connection
/// cap is plenty for typical workloads.
const POOL_MAX: u32 = 4;

/// Idle-connection floor. One warm connection per service keeps
/// the cache warm for hot reads without pinning POOL_MAX × cache
/// memory at idle.
const POOL_MIN_IDLE: u32 = 1;

/// Per-connection cache size in KiB (negative = absolute KiB
/// rather than pages). 2 MiB per connection — small caches are
/// fine because the OS page cache backs unmapped pages.
const CACHE_SIZE_KIB: i64 = -2 * 1024;

/// Per-connection mmap window cap. Lazy mapping — only resident
/// as the DB grows AND pages get touched, but the OS still bills
/// the mapping toward RSS so we keep it tight.
const MMAP_SIZE_BYTES: i64 = 16 * 1024 * 1024;

/// WAL auto-checkpoint threshold in pages. The default 1000 pages
/// (~4 MiB) is fine for throughput but means the WAL holds that
/// much memory between checkpoints. 256 pages (~1 MiB) keeps the
/// WAL bounded for a small write-throughput hit.
const WAL_AUTOCHECKPOINT_PAGES: i64 = 256;

type Pool = r2d2::Pool<SqliteConnectionManager>;
pub(crate) type Conn = PooledConnection<SqliteConnectionManager>;

/// One sqlite-backed store per AWSim instance. All accounts/regions/
/// tables share the same database, partitioned by columns. Cheap to
/// clone — backed by an Arc'd r2d2 connection pool.
#[derive(Clone)]
pub struct SqliteStore {
    inner: Arc<Inner>,
}

struct Inner {
    /// Path to the sqlite file. Kept for diagnostics + VACUUM.
    db_path: PathBuf,
    /// Pooled SQLite connections — readers never block each other in
    /// WAL mode, and we keep the pool small so per-connection memory
    /// (cache + mmap) stays bounded.
    pool: Pool,
}

impl SqliteStore {
    /// Open (or create) the sqlite file at `path` and run pending
    /// migrations. Pre-builds the connection pool so PRAGMAs are
    /// applied once per long-lived connection rather than per query.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AwsError> {
        let db_path = path.into();
        let manager = SqliteConnectionManager::file(&db_path).with_init(apply_pragmas);
        let pool = r2d2::Pool::builder()
            .max_size(POOL_MAX)
            .min_idle(Some(POOL_MIN_IDLE))
            .build(manager)
            .map_err(|e| AwsError::internal(format!("DynamoDB pool init failed: {e}")))?;
        // Migrations need a fresh `&mut Connection`. Pull one from the
        // pool, run the runner, then drop it back so the rest of the
        // pool inherits the post-migration schema.
        {
            let mut conn = pool
                .get()
                .map_err(|e| AwsError::internal(format!("DynamoDB pool acquire failed: {e}")))?;
            embedded_migrations::migrations::runner()
                .run(&mut *conn)
                .map_err(|e| AwsError::internal(format!("DynamoDB migration failed: {e}")))?;
        }
        Ok(Self {
            inner: Arc::new(Inner { db_path, pool }),
        })
    }

    /// Test-only: open a brand-new store backed by a temporary file in
    /// `std::env::temp_dir()`. We can't use `:memory:` because each
    /// rusqlite `Connection::open_in_memory()` returns an INDEPENDENT
    /// database — migrations run on one connection wouldn't be visible
    /// to subsequent reads/writes on a different connection. The temp
    /// file is unique per call (uuid-suffixed) so tests don't collide.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self, AwsError> {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-ddb-test-{id}.db"));
        Self::open(path)
    }

    /// Path to the underlying sqlite file. Used by VACUUM and tests.
    pub fn db_path(&self) -> &std::path::Path {
        &self.inner.db_path
    }

    /// Reclaim disk space after heavy DELETE / UPDATE churn. Cheap
    /// when the file is already compact; expensive when it's not, so
    /// expose this as an explicit admin operation rather than running
    /// it on every shutdown.
    pub fn vacuum(&self) -> Result<(), AwsError> {
        let conn = self.conn()?;
        conn.execute("VACUUM", []).map_err(sqlite_err)?;
        Ok(())
    }

    fn conn(&self) -> Result<Conn, AwsError> {
        self.inner
            .pool
            .get()
            .map_err(|e| AwsError::internal(format!("DynamoDB pool acquire failed: {e}")))
    }

    // -----------------------------------------------------------------
    // Item CRUD — these are the primitives the operation handlers will
    // call once we wire them up in stage 2. Each method takes a fresh
    // connection and runs in the calling thread.
    // -----------------------------------------------------------------

    /// Look up a single item by full primary key. Returns the stored
    /// `attrs_json` decoded as a JSON object, or `None` when missing.
    pub fn get_item(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        sk: &str,
    ) -> Result<Option<Value>, AwsError> {
        let conn = self.conn()?;
        let row: Option<String> = conn
            .query_row(
                "SELECT attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND pk = ?4 AND sk = ?5",
                params![account, region, table, pk, sk],
                |r| r.get(0),
            )
            .optional()
            .map_err(sqlite_err)?;
        row.map(|s| serde_json::from_str(&s).map_err(json_err))
            .transpose()
    }

    /// Upsert an item. The `gsi_keys` slice carries up to `MAX_GSI_SLOTS`
    /// `(pk, sk)` pairs in slot order — pass `(None, None)` for unused
    /// slots and for items that don't materialise into the GSI (sparse
    /// index semantics).
    #[allow(clippy::too_many_arguments)]
    pub fn put_item(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        sk: &str,
        attrs: &Value,
        gsi_keys: &[(Option<String>, Option<String>); MAX_GSI_SLOTS],
    ) -> Result<(), AwsError> {
        let conn = self.conn()?;
        let attrs_json = serde_json::to_string(attrs).map_err(json_err)?;
        conn.execute(
            "INSERT INTO items (
                account, region, table_name, pk, sk, attrs_json,
                gsi1_pk, gsi1_sk, gsi2_pk, gsi2_sk, gsi3_pk, gsi3_sk,
                gsi4_pk, gsi4_sk, gsi5_pk, gsi5_sk
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
             )
             ON CONFLICT(account, region, table_name, pk, sk) DO UPDATE SET
                attrs_json = excluded.attrs_json,
                gsi1_pk = excluded.gsi1_pk, gsi1_sk = excluded.gsi1_sk,
                gsi2_pk = excluded.gsi2_pk, gsi2_sk = excluded.gsi2_sk,
                gsi3_pk = excluded.gsi3_pk, gsi3_sk = excluded.gsi3_sk,
                gsi4_pk = excluded.gsi4_pk, gsi4_sk = excluded.gsi4_sk,
                gsi5_pk = excluded.gsi5_pk, gsi5_sk = excluded.gsi5_sk",
            params![
                account,
                region,
                table,
                pk,
                sk,
                attrs_json,
                gsi_keys[0].0,
                gsi_keys[0].1,
                gsi_keys[1].0,
                gsi_keys[1].1,
                gsi_keys[2].0,
                gsi_keys[2].1,
                gsi_keys[3].0,
                gsi_keys[3].1,
                gsi_keys[4].0,
                gsi_keys[4].1,
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Delete an item. Returns `true` if a row was actually removed.
    pub fn delete_item(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        sk: &str,
    ) -> Result<bool, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND pk = ?4 AND sk = ?5",
                params![account, region, table, pk, sk],
            )
            .map_err(sqlite_err)?;
        Ok(n > 0)
    }

    /// Row count for a table (cheap — covered by the PRIMARY KEY index).
    pub fn count_items(&self, account: &str, region: &str, table: &str) -> Result<u64, AwsError> {
        let conn = self.conn()?;
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3",
                params![account, region, table],
                |r| r.get(0),
            )
            .map_err(sqlite_err)?;
        Ok(n as u64)
    }

    /// Stream items in a single partition (Query). The visitor sees each
    /// row in (sk asc) or (sk desc) order and may stop iteration by
    /// returning `Ok(false)`. Filter and projection evaluation happens in
    /// the caller — pushing them down to SQL is impractical because
    /// DynamoDB filter expressions touch typed AttributeValues, not raw
    /// strings.
    ///
    /// `start_after_sk` is the `ExclusiveStartKey`'s sort key value; rows
    /// with that exact sk are skipped. (For tables without a sort key it
    /// is meaningless and should be `None`.)
    #[allow(clippy::too_many_arguments)]
    pub fn query_partition<F>(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        forward: bool,
        start_after_sk: Option<&str>,
        mut visit: F,
    ) -> Result<(), AwsError>
    where
        F: FnMut(&str, Value) -> Result<bool, AwsError>,
    {
        let conn = self.conn()?;
        let order = if forward { "ASC" } else { "DESC" };

        // Two query shapes — with vs. without an exclusive-start sort key.
        // Splitting the SQL keeps the parameter list straightforward and
        // avoids fiddling with NULL bindings on the comparator branch.
        let sql = match start_after_sk {
            Some(_) => {
                let cmp = if forward { ">" } else { "<" };
                format!(
                    "SELECT sk, attrs_json FROM items
                     WHERE account = ?1 AND region = ?2 AND table_name = ?3
                       AND pk = ?4 AND sk {cmp} ?5
                     ORDER BY sk {order}"
                )
            }
            None => format!(
                "SELECT sk, attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND pk = ?4
                 ORDER BY sk {order}"
            ),
        };

        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let mut rows = match start_after_sk {
            Some(start) => stmt.query(params![account, region, table, pk, start]),
            None => stmt.query(params![account, region, table, pk]),
        }
        .map_err(sqlite_err)?;

        while let Some(row) = rows.next().map_err(sqlite_err)? {
            let sk: String = row.get(0).map_err(sqlite_err)?;
            let attrs_json: String = row.get(1).map_err(sqlite_err)?;
            let attrs: Value = serde_json::from_str(&attrs_json).map_err(json_err)?;
            if !visit(&sk, attrs)? {
                break;
            }
        }
        Ok(())
    }

    /// Stream every item in a table (Scan). Items arrive in `(pk, sk)`
    /// ascending order. Returning `Ok(false)` from the visitor stops the
    /// scan; otherwise it runs to completion.
    ///
    /// `start_after` lets a caller resume from the `ExclusiveStartKey` of
    /// a prior page — rows are returned where `(pk, sk) > (start_pk,
    /// start_sk)` lexicographically.
    pub fn scan_table<F>(
        &self,
        account: &str,
        region: &str,
        table: &str,
        start_after: Option<(&str, &str)>,
        mut visit: F,
    ) -> Result<(), AwsError>
    where
        F: FnMut(&str, &str, Value) -> Result<bool, AwsError>,
    {
        let conn = self.conn()?;
        let sql = match start_after {
            Some(_) => {
                "SELECT pk, sk, attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND (pk > ?4 OR (pk = ?4 AND sk > ?5))
                 ORDER BY pk ASC, sk ASC"
            }
            None => {
                "SELECT pk, sk, attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                 ORDER BY pk ASC, sk ASC"
            }
        };

        let mut stmt = conn.prepare(sql).map_err(sqlite_err)?;
        let mut rows = if let Some((spk, ssk)) = start_after {
            stmt.query(params![account, region, table, spk, ssk])
        } else {
            stmt.query(params![account, region, table])
        }
        .map_err(sqlite_err)?;

        while let Some(row) = rows.next().map_err(sqlite_err)? {
            let pk: String = row.get(0).map_err(sqlite_err)?;
            let sk: String = row.get(1).map_err(sqlite_err)?;
            let attrs_json: String = row.get(2).map_err(sqlite_err)?;
            let attrs: Value = serde_json::from_str(&attrs_json).map_err(json_err)?;
            if !visit(&pk, &sk, attrs)? {
                break;
            }
        }
        Ok(())
    }

    /// Clear every item in a table while keeping the schema row intact.
    /// Backs the awsim-only `TruncateTable` op — DynamoDB itself doesn't
    /// support this (you'd have to DeleteTable + CreateTable), but as a
    /// dev tool it's a much faster reset for the UI's "wipe + retest"
    /// loop. Returns the number of rows removed.
    pub fn truncate_table(
        &self,
        account: &str,
        region: &str,
        table: &str,
    ) -> Result<u64, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3",
                params![account, region, table],
            )
            .map_err(sqlite_err)?;
        Ok(n as u64)
    }

    /// Drop every row for a table — used by `DeleteTable`.
    pub fn drop_table(&self, account: &str, region: &str, table: &str) -> Result<u64, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3",
                params![account, region, table],
            )
            .map_err(sqlite_err)?;
        conn.execute(
            "DELETE FROM tables
             WHERE account = ?1 AND region = ?2 AND table_name = ?3",
            params![account, region, table],
        )
        .map_err(sqlite_err)?;
        Ok(n as u64)
    }

    // -----------------------------------------------------------------
    // Table metadata storage. Schemas don't change often and are always
    // read whole, so a JSON blob keyed by (account, region, table) is
    // more ergonomic than fully normalising into separate tables.
    // -----------------------------------------------------------------

    pub fn put_table_schema(
        &self,
        account: &str,
        region: &str,
        table: &str,
        schema: &Value,
    ) -> Result<(), AwsError> {
        let conn = self.conn()?;
        let schema_json = serde_json::to_string(schema).map_err(json_err)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        conn.execute(
            "INSERT INTO tables (account, region, table_name, schema_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(account, region, table_name) DO UPDATE SET
                schema_json = excluded.schema_json",
            params![account, region, table, schema_json, now],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    pub fn get_table_schema(
        &self,
        account: &str,
        region: &str,
        table: &str,
    ) -> Result<Option<Value>, AwsError> {
        let conn = self.conn()?;
        let row: Option<String> = conn
            .query_row(
                "SELECT schema_json FROM tables
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3",
                params![account, region, table],
                |r| r.get(0),
            )
            .optional()
            .map_err(sqlite_err)?;
        row.map(|s| serde_json::from_str(&s).map_err(json_err))
            .transpose()
    }

    pub fn list_table_names(&self, account: &str, region: &str) -> Result<Vec<String>, AwsError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT table_name FROM tables
                 WHERE account = ?1 AND region = ?2
                 ORDER BY table_name",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map(params![account, region], |r| r.get::<_, String>(0))
            .map_err(sqlite_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err)
    }

    // -----------------------------------------------------------------
    // Transactional execution. The two `with_*_transaction` helpers
    // open a fresh connection, begin a sqlite transaction, hand the
    // caller a small `WriteTx` / `ReadTx` wrapper, and commit (or roll
    // back on error) when the closure returns. Any panic inside the
    // closure aborts the transaction via `Drop`.
    // -----------------------------------------------------------------

    /// Run `f` inside a single sqlite write transaction. We open with
    /// `BEGIN IMMEDIATE` so the connection acquires a RESERVED lock up
    /// front — that way a concurrent writer can't slip in between the
    /// closure's reads and writes (TransactWriteItems' phase-1/phase-2
    /// split would otherwise be racy).
    pub fn with_write_transaction<F, T>(&self, f: F) -> Result<T, AwsError>
    where
        F: FnOnce(&WriteTx<'_>) -> Result<T, AwsError>,
    {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_err)?;
        let result = {
            let wtx = WriteTx { conn: &tx };
            f(&wtx)
        };
        match result {
            Ok(val) => {
                tx.commit().map_err(sqlite_err)?;
                Ok(val)
            }
            Err(e) => {
                // `tx`'s Drop runs ROLLBACK automatically.
                Err(e)
            }
        }
    }

    /// Run `f` inside a deferred read transaction so a series of reads
    /// see a consistent snapshot. Used by TransactGetItems.
    pub fn with_read_transaction<F, T>(&self, f: F) -> Result<T, AwsError>
    where
        F: FnOnce(&ReadTx<'_>) -> Result<T, AwsError>,
    {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(sqlite_err)?;
        let result = {
            let rtx = ReadTx { conn: &tx };
            f(&rtx)
        };
        // Read txn doesn't need an explicit commit (no writes), but
        // calling commit() releases locks promptly instead of waiting
        // for Drop.
        let _ = tx.commit();
        result
    }
}

/// Read+write handle bound to an open sqlite transaction.
///
/// Mirrors a subset of `SqliteStore`'s methods so callers can do
/// `tx.put_item(...)` / `tx.delete_item(...)` and get atomic semantics.
/// Operates on the same `Connection` the transaction was started on, so
/// every statement runs against the same in-flight transaction.
pub struct WriteTx<'tx> {
    conn: &'tx Connection,
}

impl<'tx> WriteTx<'tx> {
    pub fn get_item(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        sk: &str,
    ) -> Result<Option<Value>, AwsError> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND pk = ?4 AND sk = ?5",
                params![account, region, table, pk, sk],
                |r| r.get(0),
            )
            .optional()
            .map_err(sqlite_err)?;
        row.map(|s| serde_json::from_str(&s).map_err(json_err))
            .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn put_item(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        sk: &str,
        attrs: &Value,
        gsi_keys: &[(Option<String>, Option<String>); MAX_GSI_SLOTS],
    ) -> Result<(), AwsError> {
        let attrs_json = serde_json::to_string(attrs).map_err(json_err)?;
        self.conn
            .execute(
                "INSERT INTO items (
                    account, region, table_name, pk, sk, attrs_json,
                    gsi1_pk, gsi1_sk, gsi2_pk, gsi2_sk, gsi3_pk, gsi3_sk,
                    gsi4_pk, gsi4_sk, gsi5_pk, gsi5_sk
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
                 )
                 ON CONFLICT(account, region, table_name, pk, sk) DO UPDATE SET
                    attrs_json = excluded.attrs_json,
                    gsi1_pk = excluded.gsi1_pk, gsi1_sk = excluded.gsi1_sk,
                    gsi2_pk = excluded.gsi2_pk, gsi2_sk = excluded.gsi2_sk,
                    gsi3_pk = excluded.gsi3_pk, gsi3_sk = excluded.gsi3_sk,
                    gsi4_pk = excluded.gsi4_pk, gsi4_sk = excluded.gsi4_sk,
                    gsi5_pk = excluded.gsi5_pk, gsi5_sk = excluded.gsi5_sk",
                params![
                    account,
                    region,
                    table,
                    pk,
                    sk,
                    attrs_json,
                    gsi_keys[0].0,
                    gsi_keys[0].1,
                    gsi_keys[1].0,
                    gsi_keys[1].1,
                    gsi_keys[2].0,
                    gsi_keys[2].1,
                    gsi_keys[3].0,
                    gsi_keys[3].1,
                    gsi_keys[4].0,
                    gsi_keys[4].1,
                ],
            )
            .map_err(sqlite_err)?;
        Ok(())
    }

    pub fn delete_item(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        sk: &str,
    ) -> Result<bool, AwsError> {
        let n = self
            .conn
            .execute(
                "DELETE FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND pk = ?4 AND sk = ?5",
                params![account, region, table, pk, sk],
            )
            .map_err(sqlite_err)?;
        Ok(n > 0)
    }
}

/// Read-only handle bound to a deferred sqlite transaction. Provides
/// snapshot-consistent reads across multiple `get_item` calls so
/// TransactGetItems can return a coherent view even under concurrent
/// writes.
pub struct ReadTx<'tx> {
    conn: &'tx Connection,
}

impl<'tx> ReadTx<'tx> {
    pub fn get_item(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        sk: &str,
    ) -> Result<Option<Value>, AwsError> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND pk = ?4 AND sk = ?5",
                params![account, region, table, pk, sk],
                |r| r.get(0),
            )
            .optional()
            .map_err(sqlite_err)?;
        row.map(|s| serde_json::from_str(&s).map_err(json_err))
            .transpose()
    }
}

/// Connection initialiser run by the r2d2 pool whenever it spins up
/// a new connection. Applies the same PRAGMAs the legacy per-query
/// `open_conn` did, but with a leaner memory profile — connections
/// are long-lived now, so cache + mmap budgets multiply by pool size
/// rather than concurrent-query count.
fn apply_pragmas(conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
    // WAL mode is sticky on the file, but re-setting per-connection is
    // cheap and ensures synchronous = NORMAL applies to every reader.
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
    AwsError::internal(format!("DynamoDB sqlite error: {e}"))
}

fn json_err(e: serde_json::Error) -> AwsError {
    AwsError::internal(format!("DynamoDB json error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_gsi() -> [(Option<String>, Option<String>); MAX_GSI_SLOTS] {
        Default::default()
    }

    #[test]
    fn migrations_apply_to_fresh_db() {
        let store = SqliteStore::in_memory().unwrap();
        // Smoke: a basic CRUD round-trip after migrations should just work.
        store
            .put_item(
                "acct",
                "us-east-1",
                "t",
                "pk1",
                "sk1",
                &json!({"x": 1}),
                &empty_gsi(),
            )
            .unwrap();
        let got = store
            .get_item("acct", "us-east-1", "t", "pk1", "sk1")
            .unwrap();
        assert_eq!(got, Some(json!({"x": 1})));
    }

    #[test]
    fn put_item_upserts_on_pk_sk_collision() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .put_item("a", "r", "t", "p", "s", &json!({"v": 1}), &empty_gsi())
            .unwrap();
        store
            .put_item("a", "r", "t", "p", "s", &json!({"v": 2}), &empty_gsi())
            .unwrap();
        assert_eq!(
            store.get_item("a", "r", "t", "p", "s").unwrap(),
            Some(json!({"v": 2}))
        );
        assert_eq!(store.count_items("a", "r", "t").unwrap(), 1);
    }

    #[test]
    fn isolation_across_account_region_table() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .put_item("a1", "r1", "t1", "p", "s", &json!({"x": 1}), &empty_gsi())
            .unwrap();
        store
            .put_item("a2", "r1", "t1", "p", "s", &json!({"x": 2}), &empty_gsi())
            .unwrap();
        store
            .put_item("a1", "r2", "t1", "p", "s", &json!({"x": 3}), &empty_gsi())
            .unwrap();
        store
            .put_item("a1", "r1", "t2", "p", "s", &json!({"x": 4}), &empty_gsi())
            .unwrap();
        assert_eq!(
            store.get_item("a1", "r1", "t1", "p", "s").unwrap(),
            Some(json!({"x": 1}))
        );
        assert_eq!(
            store.get_item("a2", "r1", "t1", "p", "s").unwrap(),
            Some(json!({"x": 2}))
        );
        assert_eq!(
            store.get_item("a1", "r2", "t1", "p", "s").unwrap(),
            Some(json!({"x": 3}))
        );
        assert_eq!(
            store.get_item("a1", "r1", "t2", "p", "s").unwrap(),
            Some(json!({"x": 4}))
        );
    }

    #[test]
    fn delete_returns_whether_row_existed() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .put_item("a", "r", "t", "p", "s", &json!({}), &empty_gsi())
            .unwrap();
        assert!(store.delete_item("a", "r", "t", "p", "s").unwrap());
        assert!(!store.delete_item("a", "r", "t", "p", "s").unwrap());
    }

    #[test]
    fn truncate_table_clears_items_keeps_schema() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .put_table_schema("a", "r", "t1", &json!({"TableName": "t1"}))
            .unwrap();
        for i in 0..5 {
            store
                .put_item(
                    "a",
                    "r",
                    "t1",
                    "p",
                    &format!("s{i}"),
                    &json!({}),
                    &empty_gsi(),
                )
                .unwrap();
        }
        let removed = store.truncate_table("a", "r", "t1").unwrap();
        assert_eq!(removed, 5);
        assert_eq!(store.count_items("a", "r", "t1").unwrap(), 0);
        // Schema row survives.
        assert_eq!(
            store.get_table_schema("a", "r", "t1").unwrap(),
            Some(json!({"TableName": "t1"}))
        );
    }

    #[test]
    fn drop_table_clears_only_the_named_table() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .put_item("a", "r", "t1", "p", "s", &json!({}), &empty_gsi())
            .unwrap();
        store
            .put_item("a", "r", "t2", "p", "s", &json!({}), &empty_gsi())
            .unwrap();
        let dropped = store.drop_table("a", "r", "t1").unwrap();
        assert_eq!(dropped, 1);
        assert_eq!(store.count_items("a", "r", "t1").unwrap(), 0);
        assert_eq!(store.count_items("a", "r", "t2").unwrap(), 1);
    }

    #[test]
    fn query_partition_orders_and_paginates() {
        let store = SqliteStore::in_memory().unwrap();
        for i in 0..5 {
            store
                .put_item(
                    "a",
                    "r",
                    "t",
                    "p",
                    &format!("sk{i}"),
                    &json!({"i": i}),
                    &empty_gsi(),
                )
                .unwrap();
        }
        // Forward, no start: all 5 in ascending order.
        let mut got: Vec<String> = vec![];
        store
            .query_partition("a", "r", "t", "p", true, None, |sk, _v| {
                got.push(sk.to_string());
                Ok(true)
            })
            .unwrap();
        assert_eq!(got, vec!["sk0", "sk1", "sk2", "sk3", "sk4"]);

        // Reverse, start after sk2: returns sk1, sk0.
        let mut rev: Vec<String> = vec![];
        store
            .query_partition("a", "r", "t", "p", false, Some("sk2"), |sk, _v| {
                rev.push(sk.to_string());
                Ok(true)
            })
            .unwrap();
        assert_eq!(rev, vec!["sk1", "sk0"]);

        // Visitor early-stops after collecting 2 forward.
        let mut limited: Vec<String> = vec![];
        store
            .query_partition("a", "r", "t", "p", true, None, |sk, _v| {
                limited.push(sk.to_string());
                Ok(limited.len() < 2)
            })
            .unwrap();
        assert_eq!(limited, vec!["sk0", "sk1"]);
    }

    #[test]
    fn scan_table_streams_in_order_and_resumes() {
        let store = SqliteStore::in_memory().unwrap();
        for pk in &["p1", "p2"] {
            for sk in &["s1", "s2"] {
                store
                    .put_item("a", "r", "t", pk, sk, &json!({}), &empty_gsi())
                    .unwrap();
            }
        }
        let mut got: Vec<(String, String)> = vec![];
        store
            .scan_table("a", "r", "t", None, |pk, sk, _v| {
                got.push((pk.to_string(), sk.to_string()));
                Ok(true)
            })
            .unwrap();
        assert_eq!(
            got,
            vec![
                ("p1".into(), "s1".into()),
                ("p1".into(), "s2".into()),
                ("p2".into(), "s1".into()),
                ("p2".into(), "s2".into()),
            ]
        );

        // Resume after (p1, s2): expect (p2, s1), (p2, s2).
        let mut resumed: Vec<(String, String)> = vec![];
        store
            .scan_table("a", "r", "t", Some(("p1", "s2")), |pk, sk, _v| {
                resumed.push((pk.to_string(), sk.to_string()));
                Ok(true)
            })
            .unwrap();
        assert_eq!(
            resumed,
            vec![("p2".into(), "s1".into()), ("p2".into(), "s2".into())]
        );
    }

    #[test]
    fn table_schema_round_trip() {
        let store = SqliteStore::in_memory().unwrap();
        let schema = json!({
            "TableName": "users",
            "KeySchema": [{"AttributeName": "PK", "KeyType": "HASH"}],
        });
        store.put_table_schema("a", "r", "users", &schema).unwrap();
        assert_eq!(
            store.get_table_schema("a", "r", "users").unwrap(),
            Some(schema)
        );
        assert_eq!(
            store.list_table_names("a", "r").unwrap(),
            vec!["users".to_string()]
        );
    }
}
