CREATE TABLE actor_identities (actor_identity_id TEXT PRIMARY KEY, identity_kind TEXT NOT NULL, identity TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX actor_identities_identity ON actor_identities(identity_kind, identity);
CREATE TABLE machines (machine_id TEXT PRIMARY KEY, installation_id TEXT NOT NULL, label TEXT NOT NULL, state TEXT NOT NULL, allowed_workspaces TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX machines_installation ON machines(installation_id);
CREATE TABLE machine_credentials (credential_id TEXT PRIMARY KEY, machine_id TEXT NOT NULL REFERENCES machines(machine_id), token_hash TEXT NOT NULL, expires_at TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX machine_credentials_hash ON machine_credentials(token_hash);
CREATE TABLE workspaces (workspace_id TEXT PRIMARY KEY, name TEXT NOT NULL, state TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE request_log (request_id TEXT PRIMARY KEY, caller TEXT NOT NULL, content_digest TEXT NOT NULL, answer TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX request_log_age ON request_log(created_at);
