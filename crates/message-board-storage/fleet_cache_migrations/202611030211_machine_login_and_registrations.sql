-- Durable machine facts share FleetCache's pool, migrator and transaction owner.
-- Re-enrollment forget clears named central-derived caches, never these tables.
-- UUIDv7, timestamps, enum tags and variant consistency are validated in Rust.
CREATE TABLE fleet_login (
    row_id TEXT PRIMARY KEY NOT NULL,
    enrollment_id TEXT NOT NULL UNIQUE,
    sign_in TEXT NOT NULL,
    organization_id TEXT NOT NULL,
    machine_environment TEXT NOT NULL,
    receiver_id TEXT NOT NULL,
    relay_peer_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE fleet_session_registrations (
    registration_id TEXT PRIMARY KEY NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    installation_id TEXT NOT NULL,
    binding_address TEXT NOT NULL UNIQUE,
    endpoint_service_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    checked_workspace_id TEXT NOT NULL,
    started_by_kind TEXT NOT NULL,
    started_by_service_id TEXT,
    started_by_endpoint_id TEXT,
    started_by_session_id TEXT,
    remote_start_id TEXT,
    isolation_kind TEXT NOT NULL,
    isolation_instance_id TEXT NOT NULL,
    working_directory TEXT NOT NULL,
    repository TEXT,
    commit_id TEXT,
    model TEXT,
    machine_environment TEXT NOT NULL,
    lifecycle_state TEXT NOT NULL,
    creation_state TEXT,
    session_service_id TEXT,
    session_endpoint_id TEXT,
    session_id TEXT,
    organization_id TEXT,
    abandonment_cause TEXT,
    refused_setting TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (session_service_id, session_endpoint_id, session_id)
);
