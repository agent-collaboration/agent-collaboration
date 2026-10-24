CREATE TABLE fleet_outbox_requests (
    request_id TEXT PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    request TEXT NOT NULL,
    ids TEXT NOT NULL,
    state TEXT NOT NULL
);
