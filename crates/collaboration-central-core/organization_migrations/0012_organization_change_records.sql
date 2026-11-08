-- Who changed what, written in the same transaction as each change.
CREATE TABLE organization_change_records (
    id TEXT NOT NULL PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    origin TEXT NOT NULL,
    actor_person_id TEXT,
    permission_checked TEXT,
    target_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
