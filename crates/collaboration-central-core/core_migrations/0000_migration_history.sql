CREATE TABLE IF NOT EXISTS central_schema_migrations (
    row_id TEXT NOT NULL PRIMARY KEY,
    corpus TEXT NOT NULL,
    version INTEGER NOT NULL,
    description TEXT NOT NULL,
    installed_on INTEGER NOT NULL,
    success INTEGER NOT NULL CHECK(success IN (0, 1)),
    checksum BLOB NOT NULL,
    execution_time INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(corpus, version)
);
