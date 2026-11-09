-- Every staged part gets a UUIDv7 key (owner decision 2026-11-07, R6). The old
-- (transaction_key, part_index) key stays as a unique index. Each existing part's
-- id takes its own staged_at milliseconds as the UUIDv7 time, with random bits.
-- The table is re-created under its own name (no RENAME), so the stored
-- definition is exactly the one below.
CREATE TABLE _fleet_sync_staged_parts_previous (
    transaction_key TEXT NOT NULL,
    part_index INTEGER NOT NULL,
    staged_at TEXT NOT NULL,
    payload TEXT NOT NULL
);
INSERT INTO _fleet_sync_staged_parts_previous(transaction_key, part_index, staged_at, payload)
SELECT transaction_key, part_index, staged_at, payload FROM _fleet_sync_staged_parts;
DROP TABLE _fleet_sync_staged_parts;
CREATE TABLE _fleet_sync_staged_parts (
    id TEXT NOT NULL PRIMARY KEY,
    transaction_key TEXT NOT NULL,
    part_index INTEGER NOT NULL,
    staged_at TEXT NOT NULL,
    payload TEXT NOT NULL,
    UNIQUE(transaction_key, part_index)
);
INSERT INTO _fleet_sync_staged_parts(id, transaction_key, part_index, staged_at, payload)
SELECT printf('%08x-%04x-7%03x-%04x-%012x', CAST(staged_at AS INTEGER) >> 16, CAST(staged_at AS INTEGER) & 65535, random() & 4095, 32768 | (random() & 16383), random() & 281474976710655),
    transaction_key, part_index, staged_at, payload
FROM _fleet_sync_staged_parts_previous;
DROP TABLE _fleet_sync_staged_parts_previous;
