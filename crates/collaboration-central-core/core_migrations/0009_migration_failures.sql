CREATE TABLE IF NOT EXISTS central_schema_migration_failures (
    row_id TEXT NOT NULL PRIMARY KEY,
    corpus TEXT NOT NULL,
    version TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    failed_at INTEGER NOT NULL,
    UNIQUE(corpus)
);
