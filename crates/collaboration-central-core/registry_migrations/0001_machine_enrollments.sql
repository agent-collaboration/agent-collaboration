CREATE TABLE machine_enrollments (
    machine_id TEXT NOT NULL PRIMARY KEY,
    service_id TEXT NOT NULL UNIQUE,
    machine_label TEXT NOT NULL,
    access_client_id TEXT NOT NULL UNIQUE,
    credential_hash TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    last_seen_at TEXT,
    last_revocation_operation_id TEXT,
    last_enrollment_operation_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
