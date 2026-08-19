-- Make Query on a GSI cost O(items returned) instead of O(partition).
--
-- `query_gsi_partition` orders by COALESCE(gsiN_sk, ''), which gives NULL
-- (hash-only GSIs) and '' a single total order so the resume predicate and the
-- ORDER BY agree. The V1/V2 indexes cover the bare `gsiN_sk` column, and SQLite
-- cannot satisfy an ORDER BY on an expression from an index on the underlying
-- column. Every GSI query therefore built a temp B-tree over the whole
-- partition before returning its first row, which defeated the streaming
-- `Limit` cutoff: reading 100 items from a 200k-item partition sorted all 200k.
--
-- Indexing the same expression lets the planner walk the index in order and
-- stop early. Trailing `pk, sk` carry the tiebreakers the ORDER BY also names,
-- so the sort disappears entirely rather than shrinking.
--
--   before: SEARCH ... USING INDEX idx_items_gsiN / USE TEMP B-TREE FOR ORDER BY
--   after:  SEARCH ... USING INDEX idx_items_gsiN

DROP INDEX IF EXISTS idx_items_gsi1;
CREATE INDEX IF NOT EXISTS idx_items_gsi1
    ON items (account, region, table_name, gsi1_pk, COALESCE(gsi1_sk, ''), pk, sk)
    WHERE gsi1_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi2;
CREATE INDEX IF NOT EXISTS idx_items_gsi2
    ON items (account, region, table_name, gsi2_pk, COALESCE(gsi2_sk, ''), pk, sk)
    WHERE gsi2_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi3;
CREATE INDEX IF NOT EXISTS idx_items_gsi3
    ON items (account, region, table_name, gsi3_pk, COALESCE(gsi3_sk, ''), pk, sk)
    WHERE gsi3_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi4;
CREATE INDEX IF NOT EXISTS idx_items_gsi4
    ON items (account, region, table_name, gsi4_pk, COALESCE(gsi4_sk, ''), pk, sk)
    WHERE gsi4_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi5;
CREATE INDEX IF NOT EXISTS idx_items_gsi5
    ON items (account, region, table_name, gsi5_pk, COALESCE(gsi5_sk, ''), pk, sk)
    WHERE gsi5_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi6;
CREATE INDEX IF NOT EXISTS idx_items_gsi6
    ON items (account, region, table_name, gsi6_pk, COALESCE(gsi6_sk, ''), pk, sk)
    WHERE gsi6_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi7;
CREATE INDEX IF NOT EXISTS idx_items_gsi7
    ON items (account, region, table_name, gsi7_pk, COALESCE(gsi7_sk, ''), pk, sk)
    WHERE gsi7_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi8;
CREATE INDEX IF NOT EXISTS idx_items_gsi8
    ON items (account, region, table_name, gsi8_pk, COALESCE(gsi8_sk, ''), pk, sk)
    WHERE gsi8_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi9;
CREATE INDEX IF NOT EXISTS idx_items_gsi9
    ON items (account, region, table_name, gsi9_pk, COALESCE(gsi9_sk, ''), pk, sk)
    WHERE gsi9_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi10;
CREATE INDEX IF NOT EXISTS idx_items_gsi10
    ON items (account, region, table_name, gsi10_pk, COALESCE(gsi10_sk, ''), pk, sk)
    WHERE gsi10_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi11;
CREATE INDEX IF NOT EXISTS idx_items_gsi11
    ON items (account, region, table_name, gsi11_pk, COALESCE(gsi11_sk, ''), pk, sk)
    WHERE gsi11_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi12;
CREATE INDEX IF NOT EXISTS idx_items_gsi12
    ON items (account, region, table_name, gsi12_pk, COALESCE(gsi12_sk, ''), pk, sk)
    WHERE gsi12_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi13;
CREATE INDEX IF NOT EXISTS idx_items_gsi13
    ON items (account, region, table_name, gsi13_pk, COALESCE(gsi13_sk, ''), pk, sk)
    WHERE gsi13_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi14;
CREATE INDEX IF NOT EXISTS idx_items_gsi14
    ON items (account, region, table_name, gsi14_pk, COALESCE(gsi14_sk, ''), pk, sk)
    WHERE gsi14_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi15;
CREATE INDEX IF NOT EXISTS idx_items_gsi15
    ON items (account, region, table_name, gsi15_pk, COALESCE(gsi15_sk, ''), pk, sk)
    WHERE gsi15_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi16;
CREATE INDEX IF NOT EXISTS idx_items_gsi16
    ON items (account, region, table_name, gsi16_pk, COALESCE(gsi16_sk, ''), pk, sk)
    WHERE gsi16_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi17;
CREATE INDEX IF NOT EXISTS idx_items_gsi17
    ON items (account, region, table_name, gsi17_pk, COALESCE(gsi17_sk, ''), pk, sk)
    WHERE gsi17_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi18;
CREATE INDEX IF NOT EXISTS idx_items_gsi18
    ON items (account, region, table_name, gsi18_pk, COALESCE(gsi18_sk, ''), pk, sk)
    WHERE gsi18_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi19;
CREATE INDEX IF NOT EXISTS idx_items_gsi19
    ON items (account, region, table_name, gsi19_pk, COALESCE(gsi19_sk, ''), pk, sk)
    WHERE gsi19_pk IS NOT NULL;
DROP INDEX IF EXISTS idx_items_gsi20;
CREATE INDEX IF NOT EXISTS idx_items_gsi20
    ON items (account, region, table_name, gsi20_pk, COALESCE(gsi20_sk, ''), pk, sk)
    WHERE gsi20_pk IS NOT NULL;
