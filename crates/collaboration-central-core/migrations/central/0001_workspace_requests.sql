CREATE TABLE actor_identities (actor_identity_id TEXT PRIMARY KEY, identity_kind TEXT NOT NULL, identity TEXT NOT NULL, notice_cadence TEXT NOT NULL DEFAULT 'm15', created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX actor_identities_identity ON actor_identities(identity_kind, identity);
-- machine_id is the Router install's own UUIDv7 (owner, 2026-11-18): one id for
-- the Router on the Mac, on the wire and here; central mints no machine id.
CREATE TABLE machines (machine_id TEXT PRIMARY KEY, label TEXT NOT NULL, state TEXT NOT NULL, allowed_workspaces TEXT NOT NULL, enrollment_generation INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
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
CREATE TABLE workspaces (workspace_id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', state TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
-- Names ignore surrounding ASCII spaces and ASCII letter case; SQLite lower()
-- does not perform Unicode case folding. The lookup uses this same expression.
CREATE UNIQUE INDEX workspaces_canonical_name ON workspaces(lower(trim(name)));
CREATE TABLE request_log (request_id TEXT PRIMARY KEY, caller TEXT NOT NULL, content_digest TEXT NOT NULL, answer TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX request_log_age ON request_log(created_at);
