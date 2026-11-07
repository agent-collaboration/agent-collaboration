CREATE TABLE fleet_runtime_state (
    id TEXT NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL UNIQUE,
    record TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE fleet_snapshot_install_attempts (
    id TEXT NOT NULL PRIMARY KEY,
    project TEXT NOT NULL,
    epoch TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project, epoch)
);
