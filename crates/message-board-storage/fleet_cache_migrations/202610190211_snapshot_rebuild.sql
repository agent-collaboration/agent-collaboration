CREATE TABLE fleet_cache_rebuild_records (
    row_id TEXT NOT NULL PRIMARY KEY,
    id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL
);
CREATE UNIQUE INDEX fleet_cache_rebuild_records_natural_key ON fleet_cache_rebuild_records(id, kind);
