CREATE TABLE remote_wake_requests (
    operation_id TEXT NOT NULL PRIMARY KEY,
    target_json TEXT NOT NULL,
    canonical_request BLOB NOT NULL,
    body_json TEXT NOT NULL,
    valid_until_ms INTEGER NOT NULL,
    wakeup_id TEXT UNIQUE,
    snapshot_json TEXT,
    created_at_ms INTEGER
);
