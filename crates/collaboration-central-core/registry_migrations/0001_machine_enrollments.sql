CREATE TABLE machine_enrollments (
    peer_id TEXT NOT NULL PRIMARY KEY,
    service_id TEXT NOT NULL UNIQUE,
    machine_label TEXT NOT NULL,
    access_client_id TEXT NOT NULL UNIQUE,
    credential_hash TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    last_seen TEXT,
    last_revocation_operation TEXT,
    last_enrollment_operation TEXT
);
