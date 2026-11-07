CREATE TABLE machine_control_project_progress (
    machine_id TEXT NOT NULL PRIMARY KEY,
    last_issued_position INTEGER NOT NULL,
    expired_through INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE machine_control_project_notices (
    notice_id TEXT NOT NULL PRIMARY KEY,
    machine_id TEXT NOT NULL REFERENCES machine_enrollments(machine_id),
    position INTEGER NOT NULL,
    kind TEXT NOT NULL,
    project_id TEXT,
    epoch INTEGER,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(machine_id, position)
);
CREATE INDEX machine_control_project_expiry ON machine_control_project_notices(machine_id, expires_at);
