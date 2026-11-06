CREATE TABLE lease_incarnation (
    row_id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    incarnation INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
