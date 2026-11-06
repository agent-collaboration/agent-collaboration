CREATE TABLE mail_removal_backfill (
    id TEXT PRIMARY KEY NOT NULL,
    migration_name TEXT NOT NULL UNIQUE,
    admitted_count INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
