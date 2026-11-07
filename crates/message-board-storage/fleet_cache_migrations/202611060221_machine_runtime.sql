CREATE TABLE fleet_session (
    session_id TEXT NOT NULL PRIMARY KEY,
    notice_cursor TEXT NOT NULL,
    handled_through TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE fleet_link_state (
    id TEXT NOT NULL PRIMARY KEY,
    peer_id TEXT NOT NULL UNIQUE,
    endpoint TEXT NOT NULL,
    instance TEXT NOT NULL,
    connection TEXT NOT NULL,
    revision TEXT NOT NULL,
    state_kind TEXT NOT NULL,
    state_since TEXT NOT NULL,
    connecting_attempt INTEGER,
    last_frame_at TEXT,
    refusal_kind TEXT,
    refusal_detail TEXT,
    last_drop_kind TEXT,
    last_drop_at TEXT,
    last_drop_connection TEXT,
    last_failure_kind TEXT,
    last_failure_detail TEXT,
    last_failure_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE fleet_install_attempts (
    id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    epoch TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_id, epoch)
);
