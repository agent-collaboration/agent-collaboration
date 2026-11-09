CREATE TABLE machine_enrollments (
    id TEXT NOT NULL PRIMARY KEY,
    installation_id TEXT NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL,
    end_reason TEXT,
    approved_by_person_id TEXT NOT NULL,
    permission_checked TEXT NOT NULL,
    base_generation INTEGER NOT NULL,
    code_expires_at INTEGER NOT NULL,
    machine_client_id TEXT UNIQUE,
    last_grant_at INTEGER,
    ended_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX machine_enrollments_installation ON machine_enrollments (installation_id);
CREATE UNIQUE INDEX machine_enrollments_one_active
    ON machine_enrollments (installation_id) WHERE state = 'enrolled';
CREATE TABLE machine_allowances (
    id TEXT NOT NULL PRIMARY KEY,
    installation_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (installation_id, workspace_id)
);
