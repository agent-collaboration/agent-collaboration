ALTER TABLE project_directory ADD COLUMN lease_projection TEXT;

CREATE TABLE lease_claim_reservations (
    attempt_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project_directory(project_id),
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
    project_id TEXT NOT NULL REFERENCES project_directory(project_id),
    result_kind TEXT NOT NULL,
    stored_epoch TEXT,
    stored_incarnation TEXT,
    stored_change TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(operation_id, step_name)
);
