CREATE TABLE cached_fleet_records (
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(kind, id)
);
CREATE TABLE fleet_cache_checkpoint (
    singleton INTEGER PRIMARY KEY,
    position INTEGER NOT NULL
);
INSERT INTO fleet_cache_checkpoint(singleton, position) VALUES (1, 0);
