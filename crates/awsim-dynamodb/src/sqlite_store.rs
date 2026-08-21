//! SQLite-backed storage for DynamoDB items + table metadata.
//!
//! Stage 1 of the DynamoDB-to-SQLite refactor: this module ships the
//! foundation (connection management, migrations, raw item CRUD) but
//! isn't wired into the operation handlers yet. Subsequent stages
//! migrate operations one family at a time (item -> query/scan ->
//! table metadata -> streams/transact/partiql).
//!
//! Concurrency model: rusqlite is sync. Every public method here is
//! itself sync; callers cross the async boundary by wrapping calls
//! in `tokio::task::spawn_blocking` at the operation handler layer.
//!
//! Reads and writes take separate paths, because SQLite's WAL mode
//! allows unlimited concurrent readers but exactly one writer:
//!
//!   * Reads draw a connection from a read-only pool sized to the
//!     machine. Readers never block each other or the writer.
//!   * Writes serialise through one dedicated write connection behind
//!     a `Mutex`.
//!
//! Funnelling writes through a single connection is deliberate. When
//! several connections race to write, the losers land in SQLite's
//! busy handler, which *sleeps* in escalating steps up to 100 ms while
//! still holding its connection. A userspace mutex instead hands the
//! writer off in microseconds, and readers never get caught behind a
//! sleeping writer. Contention shows up as a short queue rather than
//! as a latency cliff.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, ToSql, TransactionBehavior, params, params_from_iter,
};
use serde_json::Value;

use awsim_core::AwsError;

mod embedded_migrations {
    refinery::embed_migrations!("migrations");
}

/// Maximum number of GSIs we materialise to dedicated key columns.
/// Matches AWS's per-table GSI limit. The schema reserves
/// `gsi{1..MAX_GSI_SLOTS}_{pk,sk}` columns plus a partial index per
/// slot; raising this further would mean another schema migration.
pub const MAX_GSI_SLOTS: usize = 20;

/// An index-friendly range on the stored `sk` column, derived from a
/// Query's `KeyConditionExpression`.
///
/// **This is a narrowing hint, never the source of truth.** The full key
/// condition is still evaluated per item against typed AttributeValues,
/// so a bound that is *wider* than the real condition costs a little
/// speed and nothing else. A bound that is *narrower* would silently
/// drop matching items. When in doubt, emit no bound.
///
/// Bounds are plain string comparisons because that is how `sk` is
/// stored. They are therefore only safe for `S`-typed sort keys: a
/// numeric sort key stores `"10"` and `"9"` as text, where `"10" < "9"`.
/// [`crate::operations::query::sk_bound_from_condition`] enforces that.
#[derive(Debug, Default, Clone)]
pub struct SkBound {
    /// `(value, inclusive)` lower bound.
    pub lower: Option<(String, bool)>,
    /// `(value, inclusive)` upper bound.
    pub upper: Option<(String, bool)>,
}

impl SkBound {
    /// True when neither end is constrained, so the caller can skip
    /// threading a useless bound through.
    pub fn is_unbounded(&self) -> bool {
        self.lower.is_none() && self.upper.is_none()
    }
}

/// Resume cursor for a GSI page: the prior page's last item expressed as
/// its index sort key plus the base table primary key. The base key is the
/// tiebreaker that keeps the cursor unique even when the GSI sort key
/// repeats across items (or is absent, for a hash-only GSI). Real DynamoDB
/// includes the base primary key in a GSI query's `LastEvaluatedKey` for
/// exactly this reason.
pub struct GsiResume<'a> {
    /// GSI sort key of the boundary item, or `None` for a hash-only GSI.
    pub gsi_sk: Option<&'a str>,
    /// Base table partition key of the boundary item.
    pub base_pk: &'a str,
    /// Base table sort key of the boundary item (`""` when the base table
    /// has no sort key).
    pub base_sk: &'a str,
}

/// Build the comma-separated `gsi1_pk, gsi1_sk, ..., gsiN_pk, gsiN_sk`
/// column list for use in INSERT column declarations and DO UPDATE SET.
fn gsi_column_list() -> String {
    let mut parts = Vec::with_capacity(MAX_GSI_SLOTS * 2);
    for i in 1..=MAX_GSI_SLOTS {
        parts.push(format!("gsi{i}_pk"));
        parts.push(format!("gsi{i}_sk"));
    }
    parts.join(", ")
}

/// Build the `?N, ?N+1, ..., ?N+M-1` placeholder list for the GSI
/// columns, starting at `start` (1-indexed, since rusqlite uses ?1...).
fn gsi_placeholders(start: usize) -> String {
    (0..MAX_GSI_SLOTS * 2)
        .map(|i| format!("?{}", start + i))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build the `gsi1_pk = excluded.gsi1_pk, gsi1_sk = excluded.gsi1_sk, ...`
/// list used in the ON CONFLICT DO UPDATE clause of the put-item upsert.
fn gsi_excluded_assignments() -> String {
    let mut parts = Vec::with_capacity(MAX_GSI_SLOTS * 2);
    for i in 1..=MAX_GSI_SLOTS {
        parts.push(format!("gsi{i}_pk = excluded.gsi{i}_pk"));
        parts.push(format!("gsi{i}_sk = excluded.gsi{i}_sk"));
    }
    parts.join(", ")
}

/// Reader-pool floor and ceiling. The pool is sized to the machine
/// because WAL readers never block each other, so the only real cost
/// of another reader is its page cache. The floor keeps a
/// single-core container from serialising every read; the ceiling
/// keeps a 128-core box from pinning 128 caches.
const READER_POOL_MIN: u32 = 4;
const READER_POOL_MAX: u32 = 32;

/// Idle-connection floor for the reader pool. The pool grows on
/// demand up to [`reader_pool_size`] and shrinks back when traffic
/// subsides, so idle RSS stays close to one connection's worth.
const READER_POOL_MIN_IDLE: u32 = 1;

/// How long a caller waits for a free reader before giving up. Only
/// reachable if every pooled reader is mid-query, which means the
/// disk is the bottleneck and queueing further is pointless. Well
/// below r2d2's 30 s default so a pathological case surfaces as a
/// retryable error rather than a request that hangs for half a
/// minute.
const READER_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// Busy timeout for the write connection. In-process writes are
/// already serialised by the writer mutex, so this only covers an
/// external process holding the database file (a `sqlite3` shell, a
/// second awsim against the same `--data-dir`).
const WRITER_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Busy timeout applied to readers. WAL readers do not contend with
/// the writer, so this only covers the brief `-shm` recovery window
/// after an unclean shutdown.
const READER_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Busy timeout used only for the duration of a TRUNCATE checkpoint.
/// The checkpoint waits for readers to drain while holding the writer
/// mutex, so a long timeout would stall every write behind it. Keeping
/// it short turns "readers are still busy" into a fast `busy` result
/// that the checkpointer retries on its next tick.
const CHECKPOINT_BUSY_TIMEOUT: Duration = Duration::from_millis(250);

/// Per-connection cache size in KiB (negative = absolute KiB
/// rather than pages). 2 MiB per connection. Small caches are
/// fine because the OS page cache backs unmapped pages.
const CACHE_SIZE_KIB: i64 = -2 * 1024;

/// Per-connection mmap window cap. Lazy mapping. Only resident
/// as the DB grows AND pages get touched, but the OS still bills
/// the mapping toward RSS so we keep it tight.
const MMAP_SIZE_BYTES: i64 = 16 * 1024 * 1024;

/// WAL auto-checkpoint threshold in pages, matching SQLite's own
/// default. Every checkpoint is a write-back plus fsync that stalls
/// the writer, so checkpointing more eagerly than this costs write
/// throughput under load. `spawn_wal_checkpointer`'s periodic
/// TRUNCATE pass is what actually bounds `-wal` growth.
const WAL_AUTOCHECKPOINT_PAGES: i64 = 1000;

type Pool = r2d2::Pool<SqliteConnectionManager>;
type Reader = PooledConnection<SqliteConnectionManager>;

/// Reader-pool size for this machine, clamped to
/// `[READER_POOL_MIN, READER_POOL_MAX]`.
fn reader_pool_size() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(READER_POOL_MIN)
        .clamp(READER_POOL_MIN, READER_POOL_MAX)
}

/// Outcome of a `PRAGMA wal_checkpoint(TRUNCATE)`.
#[derive(Debug, Clone, Copy)]
pub struct WalCheckpoint {
    /// True when SQLite could not complete the checkpoint because a
    /// reader or writer still held the WAL (`busy` column = 1). The
    /// `-wal` file was not truncated this round; the caller retries on
    /// its next tick.
    pub busy: bool,
    /// Total frames in the WAL when the checkpoint started.
    pub log_frames: i64,
    /// Frames written back into the main database file.
    pub checkpointed_frames: i64,
}

/// One sqlite-backed store per AWSim instance. All accounts/regions/
/// tables share the same database, partitioned by columns. Cheap to
/// clone. Backed by an Arc'd reader pool plus a single write
/// connection.
#[derive(Clone)]
pub struct SqliteStore {
    inner: Arc<Inner>,
}

struct Inner {
    /// Path to the sqlite file. Kept for diagnostics + VACUUM.
    db_path: PathBuf,
    /// Read-only pooled connections. WAL lets these run fully
    /// concurrently with each other and with the writer.
    readers: Pool,
    /// The one connection allowed to write. SQLite permits a single
    /// writer regardless, so serialising here costs nothing and
    /// avoids the busy-handler sleep storm that concurrent writers
    /// would otherwise produce.
    writer: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (or create) the sqlite file at `path` and run pending
    /// migrations.
    ///
    /// The write connection is established and migrated first so the
    /// database, its `-wal`, and its `-shm` all exist before the
    /// read-only pool opens: a read-only connection cannot create
    /// those files itself.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AwsError> {
        let db_path = path.into();

        let mut writer = Connection::open(&db_path)
            .map_err(|e| AwsError::internal(format!("DynamoDB writer open failed: {e}")))?;
        apply_writer_pragmas(&mut writer)
            .map_err(|e| AwsError::internal(format!("DynamoDB writer pragma failed: {e}")))?;
        embedded_migrations::migrations::runner()
            .run(&mut writer)
            .map_err(|e| AwsError::internal(format!("DynamoDB migration failed: {e}")))?;

        let manager = SqliteConnectionManager::file(&db_path)
            .with_flags(
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_URI,
            )
            .with_init(apply_reader_pragmas);
        let readers = r2d2::Pool::builder()
            .max_size(reader_pool_size())
            .min_idle(Some(READER_POOL_MIN_IDLE))
            .connection_timeout(READER_ACQUIRE_TIMEOUT)
            .build(manager)
            .map_err(|e| AwsError::internal(format!("DynamoDB reader pool init failed: {e}")))?;

        Ok(Self {
            inner: Arc::new(Inner {
                db_path,
                readers,
                writer: Mutex::new(writer),
            }),
        })
    }

    /// Test-only: open a brand-new store backed by a temporary file in
    /// `std::env::temp_dir()`. We can't use `:memory:` because each
    /// rusqlite `Connection::open_in_memory()` returns an INDEPENDENT
    /// database. Migrations run on one connection wouldn't be visible
    /// to subsequent reads/writes on a different connection. The temp
    /// file is unique per call (uuid-suffixed) so tests don't collide.
    ///
    /// The name deliberately avoids the `awsim-ddb-*.db` pattern that
    /// [`crate::sweep_legacy_temp_files`] cleans up on service start:
    /// any test constructing a service used to unlink the sqlite file
    /// another test was still reading, which failed at random.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self, AwsError> {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-ddbtest-{id}.sqlite"));
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
        let conn = self.writer();
        conn.execute("VACUUM", []).map_err(sqlite_err)?;
        Ok(())
    }

    /// Force a `TRUNCATE` checkpoint: flush the WAL into the main
    /// database file and shrink the `-wal` file back to zero bytes.
    ///
    /// The per-connection `wal_autocheckpoint` only performs a PASSIVE
    /// checkpoint, which is a no-op whenever another pooled connection
    /// holds the WAL. Under a sustained write firehose (bulk imports)
    /// PASSIVE perpetually loses that race, so the `-wal` file. And
    /// the WAL index mapped alongside it. Grows without bound. A
    /// periodic explicit TRUNCATE is the hard backstop.
    ///
    /// A `busy` result means the WAL was held this round and not
    /// truncated; that is expected mid-burst and the caller simply
    /// retries on its next tick.
    pub fn checkpoint_truncate(&self) -> Result<WalCheckpoint, AwsError> {
        let conn = self.writer();
        // Shrink the busy window for the checkpoint only. TRUNCATE
        // waits for readers to drain, and it holds the writer mutex
        // while it waits, so the full `WRITER_BUSY_TIMEOUT` here would
        // stall every write behind a busy WAL.
        conn.busy_timeout(CHECKPOINT_BUSY_TIMEOUT)
            .map_err(sqlite_err)?;
        let row = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        });
        // Restore before propagating, so a failed checkpoint doesn't
        // leave the writer with a 250 ms busy window for good.
        conn.busy_timeout(WRITER_BUSY_TIMEOUT).map_err(sqlite_err)?;
        let (busy, log_frames, checkpointed_frames) = row.map_err(sqlite_err)?;
        Ok(WalCheckpoint {
            busy: busy != 0,
            log_frames,
            checkpointed_frames,
        })
    }

    /// Take a read-only connection from the pool.
    fn reader(&self) -> Result<Reader, AwsError> {
        self.inner
            .readers
            .get()
            .map_err(|e| AwsError::internal(format!("DynamoDB reader acquire failed: {e}")))
    }

    /// Take the write connection, blocking until it is free.
    ///
    /// A poisoned mutex is recovered rather than propagated: the only
    /// way to poison it is a panic inside a write, and rusqlite's
    /// `Transaction` rolls back on unwind, so the connection is left
    /// consistent. Failing every subsequent write because one
    /// operation panicked would be strictly worse.
    fn writer(&self) -> MutexGuard<'_, Connection> {
        self.inner
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // -----------------------------------------------------------------
    // Item CRUD. These are the primitives the operation handlers will
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
        let conn = self.reader()?;
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
    /// `(pk, sk)` pairs in slot order. Pass `(None, None)` for unused
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
        let conn = self.writer();
        let attrs_json = serde_json::to_string(attrs).map_err(json_err)?;
        // Build the SQL once per call. Could be cached behind OnceLock.
        // The column count never changes. But `format!` is cheap relative
        // to the round-trip and the cost shows up only on writes.
        let sql = format!(
            "INSERT INTO items (
                account, region, table_name, pk, sk, attrs_json,
                {cols}
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                {placeholders}
             )
             ON CONFLICT(account, region, table_name, pk, sk) DO UPDATE SET
                attrs_json = excluded.attrs_json,
                {assigns}",
            cols = gsi_column_list(),
            placeholders = gsi_placeholders(7),
            assigns = gsi_excluded_assignments(),
        );
        // First six positional params plus 2 per GSI slot.
        let mut bound: Vec<&dyn ToSql> = Vec::with_capacity(6 + MAX_GSI_SLOTS * 2);
        bound.push(&account);
        bound.push(&region);
        bound.push(&table);
        bound.push(&pk);
        bound.push(&sk);
        bound.push(&attrs_json);
        for slot in gsi_keys {
            bound.push(&slot.0 as &dyn ToSql);
            bound.push(&slot.1 as &dyn ToSql);
        }
        conn.execute(&sql, params_from_iter(bound))
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
        let conn = self.writer();
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

    /// Delete every item in `table` whose attribute named
    /// `ttl_attribute` is a number that fell at or before
    /// `now_secs - grace_secs`. Returns how many rows were removed.
    /// Used by the TTL sweeper.
    ///
    /// `grace_secs` mirrors AWS's "items are eventually removed"
    /// guarantee. Real DynamoDB takes up to ~48 hours to evict an
    /// expired item, and tests / production workloads sometimes need
    /// to read it back in that window. A configurable grace lets the
    /// simulator behave the same: a value of 0 deletes the moment the
    /// TTL has passed (the previous behaviour), a positive value
    /// holds the item back that many seconds before eviction.
    ///
    /// We deserialise each item's attrs JSON to inspect the TTL field
    /// because it lives inside `attrs_json` (no per-attribute index).
    /// Cheap enough for the sweeper's once-per-minute cadence. It'd
    /// be the wrong tool for a tight loop.
    pub fn delete_expired_items(
        &self,
        account: &str,
        region: &str,
        table: &str,
        ttl_attribute: &str,
        now_secs: i64,
        grace_secs: u64,
    ) -> Result<u64, AwsError> {
        let cutoff = now_secs.saturating_sub(grace_secs as i64);
        let mut victims: Vec<(String, String)> = Vec::new();
        self.scan_table(account, region, table, None, |pk, sk, attrs| {
            // DynamoDB attribute values are wire-shaped: { "N": "123" }.
            let expired = attrs
                .get(ttl_attribute)
                .and_then(|v| v.get("N"))
                .and_then(|n| n.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .map(|n| n <= cutoff)
                .unwrap_or(false);
            if expired {
                victims.push((pk.to_string(), sk.to_string()));
            }
            Ok(true)
        })?;

        let mut removed = 0u64;
        for (pk, sk) in victims {
            if self.delete_item(account, region, table, &pk, &sk)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Row count for a table (cheap. Covered by the PRIMARY KEY index).
    pub fn count_items(&self, account: &str, region: &str, table: &str) -> Result<u64, AwsError> {
        let conn = self.reader()?;
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
    /// the caller. Pushing them down to SQL is impractical because
    /// DynamoDB filter expressions touch typed AttributeValues, not raw
    /// strings.
    ///
    /// `start_after_sk` is the `ExclusiveStartKey`'s sort key value; rows
    /// with that exact sk are skipped. (For tables without a sort key it
    /// is meaningless and should be `None`.)
    /// Query a GSI partition. `slot` is the zero-based GSI index
    /// (`gsi{slot+1}_*` in the storage schema). Returns rows where the
    /// indexed `pk` column equals `pk`, ordered by the indexed `sk` then
    /// the base table primary key. `resume`, when set, continues strictly
    /// after a prior page's last item using the composite
    /// `(gsi_sk, base_pk, base_sk)` cursor, so no item is skipped or
    /// repeated even when GSI sort keys collide or are absent (hash-only
    /// GSI). A hash-only GSI stores NULL in the sk column; the cursor and
    /// ordering both `COALESCE` it to the empty string so a single total
    /// order is agreed on.
    #[allow(clippy::too_many_arguments)]
    pub fn query_gsi_partition<F>(
        &self,
        account: &str,
        region: &str,
        table: &str,
        slot: usize,
        pk: &str,
        forward: bool,
        resume: Option<GsiResume<'_>>,
        mut visit: F,
    ) -> Result<(), AwsError>
    where
        F: FnMut(&str, &str, &str, Value) -> Result<bool, AwsError>,
    {
        if slot >= MAX_GSI_SLOTS {
            return Err(AwsError::validation(format!(
                "GSI slot {slot} exceeds the {MAX_GSI_SLOTS}-slot maximum"
            )));
        }
        // The column suffix is one-based to match the migration schema.
        let i = slot + 1;
        let pk_col = format!("gsi{i}_pk");
        let sk_col = format!("gsi{i}_sk");
        // Coalesce NULL (hash-only GSI) to '' so ORDER BY and the resume
        // predicate share one total order over the index sort key.
        let sk_expr = format!("COALESCE({sk_col}, '')");
        let conn = self.reader()?;
        let order = if forward { "ASC" } else { "DESC" };
        let cmp = if forward { ">" } else { "<" };

        let sql = match &resume {
            // Lexicographic "strictly after" on the (gsi_sk, pk, sk) triple.
            Some(_) => format!(
                "SELECT pk, sk, {sk_col}, attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND {pk_col} = ?4
                   AND ( {sk_expr} {cmp} ?5
                         OR ( {sk_expr} = ?5
                              AND ( pk {cmp} ?6 OR ( pk = ?6 AND sk {cmp} ?7 ) ) ) )
                 ORDER BY {sk_expr} {order}, pk {order}, sk {order}"
            ),
            None => format!(
                "SELECT pk, sk, {sk_col}, attrs_json FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3
                   AND {pk_col} = ?4
                 ORDER BY {sk_expr} {order}, pk {order}, sk {order}"
            ),
        };

        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let mut rows = match &resume {
            Some(r) => stmt.query(params![
                account,
                region,
                table,
                pk,
                r.gsi_sk.unwrap_or(""),
                r.base_pk,
                r.base_sk
            ]),
            None => stmt.query(params![account, region, table, pk]),
        }
        .map_err(sqlite_err)?;

        while let Some(row) = rows.next().map_err(sqlite_err)? {
            let base_pk: String = row.get(0).map_err(sqlite_err)?;
            let base_sk: String = row.get(1).map_err(sqlite_err)?;
            let gsi_sk: Option<String> = row.get(2).map_err(sqlite_err)?;
            let attrs_json: String = row.get(3).map_err(sqlite_err)?;
            let attrs: Value = serde_json::from_str(&attrs_json).map_err(json_err)?;
            if !visit(&base_pk, &base_sk, gsi_sk.as_deref().unwrap_or(""), attrs)? {
                break;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn query_partition<F>(
        &self,
        account: &str,
        region: &str,
        table: &str,
        pk: &str,
        forward: bool,
        start_after_sk: Option<&str>,
        sk_bound: Option<&SkBound>,
        mut visit: F,
    ) -> Result<(), AwsError>
    where
        F: FnMut(&str, Value) -> Result<bool, AwsError>,
    {
        let conn = self.reader()?;
        let order = if forward { "ASC" } else { "DESC" };

        // Bind positionally in build order so the optional clauses below
        // can come and go without renumbering anything.
        let mut bound: Vec<&dyn ToSql> = vec![&account, &region, &table, &pk];
        let mut sql = String::from(
            "SELECT sk, attrs_json FROM items
             WHERE account = ?1 AND region = ?2 AND table_name = ?3
               AND pk = ?4",
        );

        if start_after_sk.is_some() {
            let cmp = if forward { ">" } else { "<" };
            sql.push_str(&format!(" AND sk {cmp} ?{}", bound.len() + 1));
            bound.push(&start_after_sk);
        }

        // Sort-key range pushdown. The primary key is
        // (account, region, table_name, pk, sk), so a range on `sk`
        // turns a whole-partition scan into an index seek plus a walk of
        // just the matching rows.
        if let Some(b) = sk_bound {
            if let Some((lo, inclusive)) = &b.lower {
                let cmp = if *inclusive { ">=" } else { ">" };
                sql.push_str(&format!(" AND sk {cmp} ?{}", bound.len() + 1));
                bound.push(lo);
            }
            if let Some((hi, inclusive)) = &b.upper {
                let cmp = if *inclusive { "<=" } else { "<" };
                sql.push_str(&format!(" AND sk {cmp} ?{}", bound.len() + 1));
                bound.push(hi);
            }
        }

        sql.push_str(&format!(" ORDER BY sk {order}"));

        let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
        let mut rows = stmt.query(params_from_iter(bound)).map_err(sqlite_err)?;

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
    /// a prior page. Rows are returned where `(pk, sk) > (start_pk,
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
        let conn = self.reader()?;
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
    /// Backs the awsim-only `TruncateTable` op. DynamoDB itself doesn't
    /// support this (you'd have to DeleteTable + CreateTable), but as a
    /// dev tool it's a much faster reset for the UI's "wipe + retest"
    /// loop. Returns the number of rows removed.
    pub fn truncate_table(
        &self,
        account: &str,
        region: &str,
        table: &str,
    ) -> Result<u64, AwsError> {
        let conn = self.writer();
        let n = conn
            .execute(
                "DELETE FROM items
                 WHERE account = ?1 AND region = ?2 AND table_name = ?3",
                params![account, region, table],
            )
            .map_err(sqlite_err)?;
        Ok(n as u64)
    }

    /// Drop every row for a table. Used by `DeleteTable`.
    pub fn drop_table(&self, account: &str, region: &str, table: &str) -> Result<u64, AwsError> {
        let conn = self.writer();
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
        let conn = self.writer();
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
        let conn = self.reader()?;
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
        let conn = self.reader()?;
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
    /// front. That way a concurrent writer can't slip in between the
    /// closure's reads and writes (TransactWriteItems' phase-1/phase-2
    /// split would otherwise be racy).
    pub fn with_write_transaction<F, T>(&self, f: F) -> Result<T, AwsError>
    where
        F: FnOnce(&WriteTx<'_>) -> Result<T, AwsError>,
    {
        let mut conn = self.writer();
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
        let mut conn = self.reader()?;
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
        let sql = format!(
            "INSERT INTO items (
                account, region, table_name, pk, sk, attrs_json,
                {cols}
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                {placeholders}
             )
             ON CONFLICT(account, region, table_name, pk, sk) DO UPDATE SET
                attrs_json = excluded.attrs_json,
                {assigns}",
            cols = gsi_column_list(),
            placeholders = gsi_placeholders(7),
            assigns = gsi_excluded_assignments(),
        );
        let mut bound: Vec<&dyn ToSql> = Vec::with_capacity(6 + MAX_GSI_SLOTS * 2);
        bound.push(&account);
        bound.push(&region);
        bound.push(&table);
        bound.push(&pk);
        bound.push(&sk);
        bound.push(&attrs_json);
        for slot in gsi_keys {
            bound.push(&slot.0 as &dyn ToSql);
            bound.push(&slot.1 as &dyn ToSql);
        }
        self.conn
            .execute(&sql, params_from_iter(bound))
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

/// Initialise the single write connection. This is the only place
/// `journal_mode` and `wal_autocheckpoint` are set: both are
/// write-side properties, and a read-only connection cannot change
/// either.
fn apply_writer_pragmas(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    conn.busy_timeout(WRITER_BUSY_TIMEOUT)?;
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

/// Initialiser run by the r2d2 pool whenever it spins up a new
/// read-only connection. Connections are long-lived, so the cache and
/// mmap budgets multiply by pool size rather than by concurrent-query
/// count.
fn apply_reader_pragmas(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    conn.busy_timeout(READER_BUSY_TIMEOUT)?;
    conn.execute_batch(&format!(
        "PRAGMA temp_store = MEMORY;
         PRAGMA mmap_size  = {MMAP_SIZE_BYTES};
         PRAGMA cache_size = {CACHE_SIZE_KIB};"
    ))?;
    Ok(())
}

/// Map a rusqlite failure onto an AWS error.
///
/// `SQLITE_BUSY` / `SQLITE_LOCKED` mean the database was momentarily
/// contended, not that the request was bad. With writes serialised
/// in-process these should be unreachable, but an external process
/// holding the file can still produce them. DynamoDB's own transient
/// failure shape is `InternalServerError`, which every AWS SDK
/// retries with backoff, so surface that instead of an opaque
/// internal error the caller would give up on.
fn sqlite_err(e: rusqlite::Error) -> AwsError {
    if is_contention(&e) {
        return AwsError::server_error(
            "InternalServerError",
            format!("The storage layer was busy; retry the request. ({e})"),
        );
    }
    AwsError::internal(format!("DynamoDB sqlite error: {e}"))
}

/// True when `e` is a lock-contention failure rather than a real
/// error. Matches both the plain and extended result codes.
fn is_contention(e: &rusqlite::Error) -> bool {
    use rusqlite::ErrorCode;
    matches!(
        e,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked,
                ..
            },
            _
        )
    )
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

    /// The reader/writer split exists so that concurrent load produces a
    /// queue rather than a pile of `SQLITE_BUSY` failures. Sixteen
    /// threads is comfortably more than the reader pool's floor, so this
    /// exercises both pool exhaustion and writer hand-off.
    #[test]
    fn concurrent_readers_and_writers_never_surface_lock_errors() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .put_item("a", "r", "t", "seed", "", &json!({"v": 0}), &empty_gsi())
            .unwrap();

        const THREADS: usize = 16;
        const OPS: usize = 50;

        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || {
                    for n in 0..OPS {
                        store
                            .put_item(
                                "a",
                                "r",
                                "t",
                                &format!("p{i}"),
                                &format!("s{n}"),
                                &json!({"v": n}),
                                &empty_gsi(),
                            )
                            .expect("write under contention");
                        // Interleave a read so readers and the writer are
                        // in flight against each other, not just serialised
                        // phases.
                        let seed = store
                            .get_item("a", "r", "t", "seed", "")
                            .expect("read under contention");
                        assert_eq!(seed, Some(json!({"v": 0})));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("worker thread panicked");
        }

        assert_eq!(
            store.count_items("a", "r", "t").unwrap(),
            (THREADS * OPS) as u64 + 1,
            "every write landed exactly once"
        );
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
    fn checkpoint_truncate_zeroes_wal_and_preserves_data() {
        // `in_memory()` is file-backed (temp dir) so WAL applies.
        let store = SqliteStore::in_memory().unwrap();
        for i in 0..200 {
            store
                .put_item(
                    "a",
                    "r",
                    "t",
                    "p",
                    &format!("s{i}"),
                    &json!({ "v": i }),
                    &empty_gsi(),
                )
                .unwrap();
        }
        // SQLite names the WAL `<dbpath>-wal` (suffix, not an
        // extension swap), so append rather than with_extension.
        let mut wal = store.db_path().as_os_str().to_owned();
        wal.push("-wal");
        let wal = std::path::PathBuf::from(wal);

        let cp = store.checkpoint_truncate().unwrap();
        assert!(!cp.busy, "single-threaded test should not see a busy WAL");

        // TRUNCATE shrinks the -wal file to zero (or removes it).
        let wal_len = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert_eq!(wal_len, 0, "WAL file should be truncated to 0 bytes");

        // Data written before the checkpoint is still readable.
        assert_eq!(store.count_items("a", "r", "t").unwrap(), 200);
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
            .query_partition("a", "r", "t", "p", true, None, None, |sk, _v| {
                got.push(sk.to_string());
                Ok(true)
            })
            .unwrap();
        assert_eq!(got, vec!["sk0", "sk1", "sk2", "sk3", "sk4"]);

        // Reverse, start after sk2: returns sk1, sk0.
        let mut rev: Vec<String> = vec![];
        store
            .query_partition("a", "r", "t", "p", false, Some("sk2"), None, |sk, _v| {
                rev.push(sk.to_string());
                Ok(true)
            })
            .unwrap();
        assert_eq!(rev, vec!["sk1", "sk0"]);

        // Visitor early-stops after collecting 2 forward.
        let mut limited: Vec<String> = vec![];
        store
            .query_partition("a", "r", "t", "p", true, None, None, |sk, _v| {
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

    #[test]
    fn delete_expired_items_drops_only_past_ttl() {
        let store = SqliteStore::in_memory().unwrap();
        // Three items: one expired (ttl in past), one fresh (ttl in
        // future), one with no ttl attribute at all.
        store
            .put_item(
                "a",
                "r",
                "t",
                "p",
                "expired",
                &json!({"id": {"S": "expired"}, "expires_at": {"N": "1000"}}),
                &empty_gsi(),
            )
            .unwrap();
        store
            .put_item(
                "a",
                "r",
                "t",
                "p",
                "fresh",
                &json!({"id": {"S": "fresh"}, "expires_at": {"N": "9999999999"}}),
                &empty_gsi(),
            )
            .unwrap();
        store
            .put_item(
                "a",
                "r",
                "t",
                "p",
                "no-ttl",
                &json!({"id": {"S": "no-ttl"}}),
                &empty_gsi(),
            )
            .unwrap();

        // Sweep at "now = 5000". Only "expired" qualifies.
        let removed = store
            .delete_expired_items("a", "r", "t", "expires_at", 5000, 0)
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.count_items("a", "r", "t").unwrap(), 2);
        assert!(
            store
                .get_item("a", "r", "t", "p", "expired")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_item("a", "r", "t", "p", "fresh")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .get_item("a", "r", "t", "p", "no-ttl")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn delete_expired_items_no_op_when_attribute_missing() {
        // A misconfigured table whose declared TTL attribute doesn't
        // exist on any item shouldn't delete anything.
        let store = SqliteStore::in_memory().unwrap();
        store
            .put_item(
                "a",
                "r",
                "t",
                "p",
                "1",
                &json!({"id": {"S": "1"}}),
                &empty_gsi(),
            )
            .unwrap();
        let removed = store
            .delete_expired_items("a", "r", "t", "ghost_attr", 5000, 0)
            .unwrap();
        assert_eq!(removed, 0);
        assert_eq!(store.count_items("a", "r", "t").unwrap(), 1);
    }

    #[test]
    fn delete_expired_items_honors_grace_window() {
        let store = SqliteStore::in_memory().unwrap();
        // Item expired 500 s before "now"; grace of 1000 s should keep
        // it alive (cutoff = now - grace = 5000 - 1000 = 4000, and
        // ttl=4500 > 4000 -> not yet evicted). With no grace, ttl=4500
        // < now=5000 -> swept.
        store
            .put_item(
                "a",
                "r",
                "t",
                "p",
                "soon",
                &json!({"id": {"S": "soon"}, "expires_at": {"N": "4500"}}),
                &empty_gsi(),
            )
            .unwrap();

        let removed = store
            .delete_expired_items("a", "r", "t", "expires_at", 5000, 1000)
            .unwrap();
        assert_eq!(removed, 0);
        assert!(
            store
                .get_item("a", "r", "t", "p", "soon")
                .unwrap()
                .is_some(),
            "item within grace window must remain"
        );

        // Bump "now" past ttl + grace (5500 > 4500 + 1000 = 5500): equal,
        // so the boundary case still evicts (ttl + grace == now -> ttl <=
        // now - grace).
        let removed = store
            .delete_expired_items("a", "r", "t", "expires_at", 5500, 1000)
            .unwrap();
        assert_eq!(removed, 1);
    }

    #[test]
    fn put_and_query_high_slot_gsi() {
        // The schema reserves up to MAX_GSI_SLOTS dedicated key columns;
        // exercise one past slot 5 to make sure the dynamic-SQL upsert
        // and the query path both address the higher-numbered columns
        // correctly. Slot 10 is the test target (gsi11_pk / gsi11_sk).
        let store = SqliteStore::in_memory().unwrap();
        let mut keys: [(Option<String>, Option<String>); MAX_GSI_SLOTS] = Default::default();
        keys[10] = (Some("tenant-a".into()), Some("2024-12-01".into()));
        store
            .put_item(
                "acct",
                "us-east-1",
                "t",
                "pk1",
                "sk1",
                &json!({"id": {"S": "1"}}),
                &keys,
            )
            .unwrap();

        let mut hit_count = 0u32;
        store
            .query_gsi_partition(
                "acct",
                "us-east-1",
                "t",
                10,
                "tenant-a",
                true,
                None,
                |_pk, _sk, _gsi_sk, _attrs| {
                    hit_count += 1;
                    Ok(true)
                },
            )
            .unwrap();
        assert_eq!(hit_count, 1, "slot 10 lookup should find the seeded row");
    }

    #[test]
    fn query_at_max_slot_index_works() {
        // The very last legal slot. Useful as a boundary regression
        // guard so a future bump of MAX_GSI_SLOTS doesn't quietly drop
        // the upper-edge slot from the column list.
        let store = SqliteStore::in_memory().unwrap();
        let last = MAX_GSI_SLOTS - 1;
        let mut keys: [(Option<String>, Option<String>); MAX_GSI_SLOTS] = Default::default();
        keys[last] = (Some("partition".into()), Some("range".into()));
        store
            .put_item(
                "acct",
                "us-east-1",
                "t",
                "pk1",
                "sk1",
                &json!({"id": {"S": "1"}}),
                &keys,
            )
            .unwrap();
        let mut found = 0u32;
        store
            .query_gsi_partition(
                "acct",
                "us-east-1",
                "t",
                last,
                "partition",
                true,
                None,
                |_pk, _sk, _gsi_sk, _attrs| {
                    found += 1;
                    Ok(true)
                },
            )
            .unwrap();
        assert_eq!(found, 1);
    }
}
