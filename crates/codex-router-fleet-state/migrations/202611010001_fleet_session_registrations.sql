-- codex-router-fleet-state migration: 202611010001_fleet_session_registrations.sql
-- Public launch/registration facts and a private bearer binding, never a machine token or key.
-- Rust owns lifecycle/domain validation and validates the optional SessionRef as one value.
CREATE TABLE fleet_session_registrations (
    registration_id TEXT PRIMARY KEY NOT NULL,
    operation_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    binding_address TEXT NOT NULL,
    session_service_id TEXT,
    session_endpoint_id TEXT,
    session_id TEXT,
    lifecycle_json TEXT NOT NULL
);

CREATE UNIQUE INDEX fleet_session_registration_operation
    ON fleet_session_registrations (operation_id);
CREATE UNIQUE INDEX fleet_session_registration_binding
    ON fleet_session_registrations (binding_address);
CREATE UNIQUE INDEX fleet_session_registration_conversation
    ON fleet_session_registrations (session_service_id, session_endpoint_id, session_id);
