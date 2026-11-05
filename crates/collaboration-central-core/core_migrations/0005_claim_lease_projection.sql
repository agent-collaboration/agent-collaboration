ALTER TABLE fleet_project_directory ADD COLUMN lease_epoch TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_change TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_incarnation TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_changed_at INTEGER;
ALTER TABLE fleet_project_directory ADD COLUMN lease_lifecycle_kind TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_origin_kind TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_origin_epoch TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_origin_sequence TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_import_digest TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_retiring_attempt_id TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_retired_at INTEGER;
ALTER TABLE fleet_project_directory ADD COLUMN lease_holder_machine_id TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_holder_service_id TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN lease_expires_at INTEGER;
ALTER TABLE fleet_project_directory ADD COLUMN lease_digest TEXT;

CREATE TABLE lease_claim_reservations (
    attempt_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES fleet_project_directory(project_id),
    state_kind TEXT NOT NULL,
    outcome TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE lease_claim_settlement_receipts (
    receipt_id TEXT NOT NULL PRIMARY KEY,
    operation_id TEXT NOT NULL,
    step_name TEXT NOT NULL,
    attempt_id TEXT NOT NULL REFERENCES lease_claim_reservations(attempt_id),
    project_id TEXT NOT NULL REFERENCES fleet_project_directory(project_id),
    result_kind TEXT NOT NULL,
    stored_epoch TEXT,
    stored_incarnation TEXT,
    stored_change TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(operation_id, step_name)
);
