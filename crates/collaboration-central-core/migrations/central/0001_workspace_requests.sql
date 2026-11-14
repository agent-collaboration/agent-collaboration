CREATE TABLE actor_identities (actor_identity_id TEXT PRIMARY KEY, identity_kind TEXT NOT NULL, identity TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX actor_identities_identity ON actor_identities(identity_kind, identity);
CREATE TABLE machines (machine_id TEXT PRIMARY KEY, installation_id TEXT NOT NULL, label TEXT NOT NULL, state TEXT NOT NULL, allowed_workspaces TEXT NOT NULL, enrollment_generation INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX machines_installation ON machines(installation_id);
CREATE TABLE machine_approvals (
    approval_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    machine_id TEXT NOT NULL REFERENCES machines(machine_id),
    label TEXT NOT NULL,
    allowed_workspaces TEXT NOT NULL,
    state TEXT NOT NULL,
    end_reason TEXT,
    approved_by_person_id TEXT NOT NULL,
    permission_checked TEXT NOT NULL,
    base_generation INTEGER NOT NULL,
    code_expires_at TEXT NOT NULL,
    machine_client_id TEXT UNIQUE,
    last_grant_at TEXT,
    ended_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX machine_approvals_enrolled ON machine_approvals(machine_id) WHERE state='enrolled';
CREATE TABLE machine_credentials (credential_id TEXT PRIMARY KEY, machine_id TEXT NOT NULL REFERENCES machines(machine_id), token_hash TEXT NOT NULL, expires_at TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX machine_credentials_hash ON machine_credentials(token_hash);
CREATE TABLE workspaces (workspace_id TEXT PRIMARY KEY, name TEXT NOT NULL, state TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE request_log (request_id TEXT PRIMARY KEY, caller TEXT NOT NULL, content_digest TEXT NOT NULL, answer TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX request_log_age ON request_log(created_at);
