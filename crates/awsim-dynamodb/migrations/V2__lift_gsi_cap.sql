-- Lift the per-row GSI column count from 5 to 20, matching the AWS
-- per-table GSI limit. SQLite supports adding columns one at a time
-- via ALTER TABLE; we add them in order so query code can address
-- them by ordinal.
--
-- Indexes mirror the V1 pattern: one partial index per slot, scoped
-- to the (account, region, table_name) prefix and only covering rows
-- that materialise into that index.

ALTER TABLE items ADD COLUMN gsi6_pk  TEXT;
ALTER TABLE items ADD COLUMN gsi6_sk  TEXT;
ALTER TABLE items ADD COLUMN gsi7_pk  TEXT;
ALTER TABLE items ADD COLUMN gsi7_sk  TEXT;
ALTER TABLE items ADD COLUMN gsi8_pk  TEXT;
ALTER TABLE items ADD COLUMN gsi8_sk  TEXT;
ALTER TABLE items ADD COLUMN gsi9_pk  TEXT;
ALTER TABLE items ADD COLUMN gsi9_sk  TEXT;
ALTER TABLE items ADD COLUMN gsi10_pk TEXT;
ALTER TABLE items ADD COLUMN gsi10_sk TEXT;
ALTER TABLE items ADD COLUMN gsi11_pk TEXT;
ALTER TABLE items ADD COLUMN gsi11_sk TEXT;
ALTER TABLE items ADD COLUMN gsi12_pk TEXT;
ALTER TABLE items ADD COLUMN gsi12_sk TEXT;
ALTER TABLE items ADD COLUMN gsi13_pk TEXT;
ALTER TABLE items ADD COLUMN gsi13_sk TEXT;
ALTER TABLE items ADD COLUMN gsi14_pk TEXT;
ALTER TABLE items ADD COLUMN gsi14_sk TEXT;
ALTER TABLE items ADD COLUMN gsi15_pk TEXT;
ALTER TABLE items ADD COLUMN gsi15_sk TEXT;
ALTER TABLE items ADD COLUMN gsi16_pk TEXT;
ALTER TABLE items ADD COLUMN gsi16_sk TEXT;
ALTER TABLE items ADD COLUMN gsi17_pk TEXT;
ALTER TABLE items ADD COLUMN gsi17_sk TEXT;
ALTER TABLE items ADD COLUMN gsi18_pk TEXT;
ALTER TABLE items ADD COLUMN gsi18_sk TEXT;
ALTER TABLE items ADD COLUMN gsi19_pk TEXT;
ALTER TABLE items ADD COLUMN gsi19_sk TEXT;
ALTER TABLE items ADD COLUMN gsi20_pk TEXT;
ALTER TABLE items ADD COLUMN gsi20_sk TEXT;

CREATE INDEX IF NOT EXISTS idx_items_gsi6
    ON items (account, region, table_name, gsi6_pk, gsi6_sk)
    WHERE gsi6_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi7
    ON items (account, region, table_name, gsi7_pk, gsi7_sk)
    WHERE gsi7_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi8
    ON items (account, region, table_name, gsi8_pk, gsi8_sk)
    WHERE gsi8_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi9
    ON items (account, region, table_name, gsi9_pk, gsi9_sk)
    WHERE gsi9_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi10
    ON items (account, region, table_name, gsi10_pk, gsi10_sk)
    WHERE gsi10_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi11
    ON items (account, region, table_name, gsi11_pk, gsi11_sk)
    WHERE gsi11_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi12
    ON items (account, region, table_name, gsi12_pk, gsi12_sk)
    WHERE gsi12_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi13
    ON items (account, region, table_name, gsi13_pk, gsi13_sk)
    WHERE gsi13_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi14
    ON items (account, region, table_name, gsi14_pk, gsi14_sk)
    WHERE gsi14_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi15
    ON items (account, region, table_name, gsi15_pk, gsi15_sk)
    WHERE gsi15_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi16
    ON items (account, region, table_name, gsi16_pk, gsi16_sk)
    WHERE gsi16_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi17
    ON items (account, region, table_name, gsi17_pk, gsi17_sk)
    WHERE gsi17_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi18
    ON items (account, region, table_name, gsi18_pk, gsi18_sk)
    WHERE gsi18_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi19
    ON items (account, region, table_name, gsi19_pk, gsi19_sk)
    WHERE gsi19_pk IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_gsi20
    ON items (account, region, table_name, gsi20_pk, gsi20_sk)
    WHERE gsi20_pk IS NOT NULL;
