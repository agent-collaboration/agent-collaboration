-- At most one row: which organization this Fleet store belongs to. Only the
-- private provisioning link writes it; the applying transaction enforces the
-- single row (the primary key alone would admit a second organization).
CREATE TABLE organization_identity (
    id TEXT NOT NULL PRIMARY KEY,
    organization_statement TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
