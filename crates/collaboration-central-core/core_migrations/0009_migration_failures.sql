CREATE TABLE IF NOT EXISTS central_schema_migration_failures (
    row_id TEXT NOT NULL PRIMARY KEY,
    corpus TEXT NOT NULL,
    version TEXT NOT NULL,
    reason TEXT NOT NULL,
    at TEXT NOT NULL,
    UNIQUE(corpus)
);
