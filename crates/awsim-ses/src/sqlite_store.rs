//! SQLite-backed storage for SES outbound emails. Replaces the
//! per-region `DashMap<message_id, SentEmail>` that grew unbounded
//! and vanished on every restart.

use std::path::PathBuf;
use std::sync::Arc;

use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, params};

use awsim_core::AwsError;

use crate::state::SentEmail;

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

/// Row returned by `list*` queries, includes the account/region the
/// email was sent under so callers don't have to thread that
/// through separately.
#[derive(Debug, Clone)]
pub struct SentEmailRow {
    pub account: String,
    pub region: String,
    pub email: SentEmail,
}

impl SqliteStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AwsError> {
        let db_path = path.into();
        let manager = SqliteConnectionManager::file(&db_path).with_init(apply_pragmas);
        let pool = r2d2::Pool::builder()
            .max_size(POOL_MAX)
            .min_idle(Some(POOL_MIN_IDLE))
            .build(manager)
            .map_err(|e| AwsError::internal(format!("SES pool init failed: {e}")))?;
        {
            let conn = pool
                .get()
                .map_err(|e| AwsError::internal(format!("SES pool acquire failed: {e}")))?;
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
            .map_err(|e| AwsError::internal(format!("SES pool acquire failed: {e}")))
    }

    /// Persist a single outbound email.
    pub fn put_email(
        &self,
        account: &str,
        region: &str,
        email: &SentEmail,
    ) -> Result<(), AwsError> {
        let to_json = serde_json::to_string(&email.to).unwrap_or_else(|_| "[]".into());
        let cc_json = serde_json::to_string(&email.cc).unwrap_or_else(|_| "[]".into());
        let bcc_json = serde_json::to_string(&email.bcc).unwrap_or_else(|_| "[]".into());
        let tags_json = serde_json::to_string(&email.tags).unwrap_or_else(|_| "[]".into());
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO sent_emails
             (account, region, message_id, sender, to_json, cc_json, bcc_json,
              subject, body_text, body_html, raw, sent_at,
              configuration_set_name, tags_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                account,
                region,
                &email.message_id,
                &email.from,
                &to_json,
                &cc_json,
                &bcc_json,
                email.subject.as_deref(),
                email.body_text.as_deref(),
                email.body_html.as_deref(),
                email.raw.as_deref(),
                email.sent_at as i64,
                email.configuration_set_name.as_deref(),
                &tags_json,
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Snapshot every email, newest-first. Used by the admin endpoint.
    pub fn list_all(&self) -> Result<Vec<SentEmailRow>, AwsError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT account, region, message_id, sender, to_json, cc_json, bcc_json,
                        subject, body_text, body_html, raw, sent_at,
                        configuration_set_name, tags_json
                 FROM sent_emails
                 ORDER BY sent_at DESC, message_id ASC",
            )
            .map_err(sqlite_err)?;
        let rows = stmt.query_map([], row_to_email).map_err(sqlite_err)?;
        let out: Result<Vec<_>, _> = rows.collect();
        out.map_err(sqlite_err)
    }

    /// Delete emails whose `sent_at < cutoff_secs`. Used by the
    /// retention sweep so the SES outbox doesn't grow forever.
    pub fn trim_older_than(&self, cutoff_secs: i64) -> Result<usize, AwsError> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "DELETE FROM sent_emails WHERE sent_at < ?1",
                params![cutoff_secs],
            )
            .map_err(sqlite_err)?;
        Ok(n)
    }

    pub fn total_rows(&self) -> Result<u64, AwsError> {
        let conn = self.conn()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM sent_emails", [], |r| r.get(0))
            .map_err(sqlite_err)?;
        Ok(n as u64)
    }
}

fn row_to_email(row: &rusqlite::Row<'_>) -> rusqlite::Result<SentEmailRow> {
    let to_json: String = row.get(4)?;
    let cc_json: String = row.get(5)?;
    let bcc_json: String = row.get(6)?;
    let to: Vec<String> = serde_json::from_str(&to_json).unwrap_or_default();
    let cc: Vec<String> = serde_json::from_str(&cc_json).unwrap_or_default();
    let bcc: Vec<String> = serde_json::from_str(&bcc_json).unwrap_or_default();
    let sent_at: i64 = row.get(11)?;
    let configuration_set_name: Option<String> = row.get(12).ok();
    let tags_json: Option<String> = row.get(13).ok();
    let tags: Vec<(String, String)> = tags_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    Ok(SentEmailRow {
        account: row.get(0)?,
        region: row.get(1)?,
        email: SentEmail {
            message_id: row.get(2)?,
            from: row.get(3)?,
            to,
            cc,
            bcc,
            reply_to: Vec::new(),
            subject: row.get(7)?,
            body_text: row.get(8)?,
            body_html: row.get(9)?,
            raw: row.get(10)?,
            sent_at: sent_at.max(0) as u64,
            configuration_set_name,
            tags,
        },
    })
}

fn init_schema(conn: &Connection) -> Result<(), AwsError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sent_emails (
             account TEXT NOT NULL,
             region TEXT NOT NULL,
             message_id TEXT NOT NULL,
             sender TEXT NOT NULL,
             to_json TEXT NOT NULL,
             cc_json TEXT NOT NULL,
             bcc_json TEXT NOT NULL,
             subject TEXT,
             body_text TEXT,
             body_html TEXT,
             raw TEXT,
             sent_at INTEGER NOT NULL,
             PRIMARY KEY (account, region, message_id)
         );
         CREATE INDEX IF NOT EXISTS sent_emails_sent_at
             ON sent_emails (sent_at);",
    )
    .map_err(sqlite_err)?;
    // Late-added columns. SQLite tolerates ALTER TABLE for non-NULL
    // additions when the column is nullable, so re-running this is a
    // no-op on schemas that already carry them.
    add_column_if_missing(conn, "sent_emails", "configuration_set_name", "TEXT")?;
    add_column_if_missing(conn, "sent_emails", "tags_json", "TEXT")?;
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> Result<(), AwsError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(sqlite_err)?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(sqlite_err)?
        .filter_map(Result::ok)
        .collect();
    if !cols.iter().any(|c| c == column) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
            [],
        )
        .map_err(sqlite_err)?;
    }
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
    AwsError::internal(format!("SES sqlite error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SqliteStore {
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("awsim-ses-test-{id}.db"));
        SqliteStore::open(path).unwrap()
    }

    fn email(id: &str, ts: u64) -> SentEmail {
        SentEmail {
            message_id: id.into(),
            from: "alice@example.com".into(),
            to: vec!["bob@example.com".into()],
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            subject: Some("hi".into()),
            body_text: Some("hello".into()),
            body_html: None,
            raw: None,
            sent_at: ts,
            configuration_set_name: None,
            tags: vec![],
        }
    }

    #[test]
    fn put_and_list_newest_first() {
        let s = store();
        s.put_email("a", "us-east-1", &email("m1", 100)).unwrap();
        s.put_email("a", "us-east-1", &email("m2", 200)).unwrap();
        s.put_email("a", "us-east-1", &email("m3", 150)).unwrap();
        let rows = s.list_all().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].email.message_id, "m2");
        assert_eq!(rows[1].email.message_id, "m3");
        assert_eq!(rows[2].email.message_id, "m1");
    }

    #[test]
    fn trim_drops_old() {
        let s = store();
        s.put_email("a", "us-east-1", &email("old", 50)).unwrap();
        s.put_email("a", "us-east-1", &email("new", 500)).unwrap();
        let removed = s.trim_older_than(100).unwrap();
        assert_eq!(removed, 1);
        let rows = s.list_all().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].email.message_id, "new");
    }
}
