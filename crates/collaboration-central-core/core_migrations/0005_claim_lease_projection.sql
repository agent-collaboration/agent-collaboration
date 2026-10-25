ALTER TABLE project_directory ADD COLUMN lease_projection TEXT;

CREATE TABLE lease_incarnation_fence (
    fence_id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    incarnation TEXT NOT NULL
);

CREATE TABLE claim_reservations (
    attempt_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    state TEXT NOT NULL
);

CREATE TABLE claim_settlement_receipts (
    receipt_id TEXT NOT NULL PRIMARY KEY,
    operation_id TEXT NOT NULL,
    step_name TEXT NOT NULL,
    attempt_id TEXT NOT NULL REFERENCES claim_reservations(attempt_id),
    project_id TEXT NOT NULL,
    result TEXT NOT NULL,
    UNIQUE(operation_id, step_name)
);
