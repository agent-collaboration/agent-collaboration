CREATE TABLE IF NOT EXISTS central_schema_migrations (
    row_id TEXT NOT NULL PRIMARY KEY,
    corpus TEXT NOT NULL,
    version TEXT NOT NULL,
    checksum TEXT NOT NULL,
    UNIQUE(corpus, version)
);
