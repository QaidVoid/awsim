-- AWSim DynamoDB SQLite-backed storage
--
-- One database per AWSim instance, multi-tenant via (account, region) cols.
-- Per-account/region/table sharding could come later if contention bites,
-- but a single database with WAL mode handles thousands of concurrent
-- readers + a single writer comfortably.
--
-- All PRAGMAs (journal_mode, synchronous, temp_store, mmap_size, cache_size)
-- are applied per-connection in open_conn() — they can't run inside the
-- transaction refinery wraps each migration in.

-- Item storage. One row = one DynamoDB item. `attrs_json` is the full
-- item as DynamoDB-serialised JSON ({ "AttrName": { "S": "value" } }).
-- The discrete `pk` / `sk` columns are extracted at write time so we can
-- index them and serve `Query` against PK + SK efficiently.
CREATE TABLE IF NOT EXISTS items (
    account     TEXT NOT NULL,
    region      TEXT NOT NULL,
    table_name  TEXT NOT NULL,
    pk          TEXT NOT NULL,
    sk          TEXT NOT NULL DEFAULT '',  -- empty when the table has no sort key
    attrs_json  TEXT NOT NULL,
    -- GSI key columns are projected at write time when the item has the
    -- attributes the GSI keys on. NULL otherwise = item drops out of the
    -- index (matches DynamoDB sparse-index semantics).
    gsi1_pk     TEXT,
    gsi1_sk     TEXT,
    gsi2_pk     TEXT,
    gsi2_sk     TEXT,
    gsi3_pk     TEXT,
    gsi3_sk     TEXT,
    gsi4_pk     TEXT,
    gsi4_sk     TEXT,
    gsi5_pk     TEXT,
    gsi5_sk     TEXT,
    PRIMARY KEY (account, region, table_name, pk, sk)
) WITHOUT ROWID;

-- One index per GSI slot. Sparse — items where the GSI key is NULL are
-- excluded automatically by the WHERE clause on writes that look them up.
CREATE INDEX IF NOT EXISTS idx_items_gsi1
    ON items (account, region, table_name, gsi1_pk, gsi1_sk)
    WHERE gsi1_pk IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_items_gsi2
    ON items (account, region, table_name, gsi2_pk, gsi2_sk)
    WHERE gsi2_pk IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_items_gsi3
    ON items (account, region, table_name, gsi3_pk, gsi3_sk)
    WHERE gsi3_pk IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_items_gsi4
    ON items (account, region, table_name, gsi4_pk, gsi4_sk)
    WHERE gsi4_pk IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_items_gsi5
    ON items (account, region, table_name, gsi5_pk, gsi5_sk)
    WHERE gsi5_pk IS NOT NULL;

-- Table metadata (key schema, attribute defs, GSI definitions, etc.).
-- Stored as a single JSON blob per table — these change rarely and are
-- always read whole, so a JSON column is more ergonomic than fully
-- normalising into separate tables.
CREATE TABLE IF NOT EXISTS tables (
    account     TEXT NOT NULL,
    region      TEXT NOT NULL,
    table_name  TEXT NOT NULL,
    schema_json TEXT NOT NULL,
    created_at  INTEGER NOT NULL,  -- unix epoch seconds
    PRIMARY KEY (account, region, table_name)
) WITHOUT ROWID;
