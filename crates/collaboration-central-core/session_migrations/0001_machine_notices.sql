CREATE TABLE machine_notice_state (
    state_id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    last_position INTEGER NOT NULL,
    pruned_through INTEGER NOT NULL,
    peer_id TEXT
);
CREATE TABLE machine_notices (
    notice_id TEXT NOT NULL PRIMARY KEY,
    position INTEGER NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX machine_notice_expiry ON machine_notices(expires_at);
