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

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use awsim_core::AwsError;

mod embedded_migrations {
    refinery::embed_migrations!("migrations");
}

/// Maximum number of GSIs we materialise to dedicated key columns.
/// DynamoDB's hard cap is 20; this covers every realistic use case
/// while keeping the row width modest.
pub const MAX_GSI_SLOTS: usize = 5;

/// One sqlite-backed store per AWSim instance. All accounts/regions/
/// tables share the same database, partitioned by columns. Cheap to
/// clone — internal connection management is via thread-local handles
/// in rusqlite.
#[derive(Clone)]
pub struct SqliteStore {
    inner: Arc<Inner>,
}

struct Inner {
    /// Path to the sqlite file (or `":memory:"` for ephemeral).
    db_path: PathBuf,
}

impl SqliteStore {
    /// Open (or create) the sqlite file at `path` and run pending
    /// migrations. Use `":memory:"` for an ephemeral store — useful in
    /// tests and when AWSim is started without `--data-dir`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AwsError> {
        let db_path = path.into();
        let mut conn = open_conn(&db_path)?;
        embedded_migrations::migrations::runner()
            .run(&mut conn)
            .map_err(|e| {
                AwsError::internal(format!("DynamoDB migration failed: {e}"))
            })?;
        Ok(Self {
            inner: Arc::new(Inner { db_path }),
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

    fn conn(&self) -> Result<Connection, AwsError> {
        open_conn(&self.inner.db_path)
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
                account, region, table, pk, sk, attrs_json,
                gsi_keys[0].0, gsi_keys[0].1,
                gsi_keys[1].0, gsi_keys[1].1,
                gsi_keys[2].0, gsi_keys[2].1,
                gsi_keys[3].0, gsi_keys[3].1,
                gsi_keys[4].0, gsi_keys[4].1,
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
    pub fn count_items(
        &self,
        account: &str,
        region: &str,
        table: &str,
    ) -> Result<u64, AwsError> {
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

    /// Drop every row for a table — used by `DeleteTable`.
    pub fn drop_table(
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

    pub fn list_table_names(
        &self,
        account: &str,
        region: &str,
    ) -> Result<Vec<String>, AwsError> {
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
}

fn open_conn(path: &PathBuf) -> Result<Connection, AwsError> {
    let conn = if path.as_os_str() == ":memory:" {
        Connection::open_in_memory()
    } else {
        Connection::open(path)
    }
    .map_err(sqlite_err)?;
    // Apply PRAGMAs per-connection. `journal_mode = WAL` and
    // `synchronous = NORMAL` are sticky once set on the database, but
    // re-issuing them is cheap and harmless. The rest are session
    // PRAGMAs that need to be set on every fresh connection.
    //
    // `:memory:` databases can't switch journal mode (WAL needs a file),
    // so we skip those PRAGMAs there.
    if path.as_os_str() != ":memory:" {
        conn.pragma_update(None, "journal_mode", "WAL").map_err(sqlite_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(sqlite_err)?;
    }
    conn.execute_batch(
        "PRAGMA temp_store = MEMORY;
         PRAGMA mmap_size  = 268435456;
         PRAGMA cache_size = -65536;",
    )
    .map_err(sqlite_err)?;
    Ok(conn)
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
            .put_item("acct", "us-east-1", "t", "pk1", "sk1", &json!({"x": 1}), &empty_gsi())
            .unwrap();
        let got = store.get_item("acct", "us-east-1", "t", "pk1", "sk1").unwrap();
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
        store.put_item("a1", "r1", "t1", "p", "s", &json!({"x": 1}), &empty_gsi()).unwrap();
        store.put_item("a2", "r1", "t1", "p", "s", &json!({"x": 2}), &empty_gsi()).unwrap();
        store.put_item("a1", "r2", "t1", "p", "s", &json!({"x": 3}), &empty_gsi()).unwrap();
        store.put_item("a1", "r1", "t2", "p", "s", &json!({"x": 4}), &empty_gsi()).unwrap();
        assert_eq!(store.get_item("a1", "r1", "t1", "p", "s").unwrap(), Some(json!({"x": 1})));
        assert_eq!(store.get_item("a2", "r1", "t1", "p", "s").unwrap(), Some(json!({"x": 2})));
        assert_eq!(store.get_item("a1", "r2", "t1", "p", "s").unwrap(), Some(json!({"x": 3})));
        assert_eq!(store.get_item("a1", "r1", "t2", "p", "s").unwrap(), Some(json!({"x": 4})));
    }

    #[test]
    fn delete_returns_whether_row_existed() {
        let store = SqliteStore::in_memory().unwrap();
        store.put_item("a", "r", "t", "p", "s", &json!({}), &empty_gsi()).unwrap();
        assert!(store.delete_item("a", "r", "t", "p", "s").unwrap());
        assert!(!store.delete_item("a", "r", "t", "p", "s").unwrap());
    }

    #[test]
    fn drop_table_clears_only_the_named_table() {
        let store = SqliteStore::in_memory().unwrap();
        store.put_item("a", "r", "t1", "p", "s", &json!({}), &empty_gsi()).unwrap();
        store.put_item("a", "r", "t2", "p", "s", &json!({}), &empty_gsi()).unwrap();
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
                .put_item("a", "r", "t", "p", &format!("sk{i}"), &json!({"i": i}), &empty_gsi())
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
        assert_eq!(store.get_table_schema("a", "r", "users").unwrap(), Some(schema));
        assert_eq!(store.list_table_names("a", "r").unwrap(), vec!["users".to_string()]);
    }
}
