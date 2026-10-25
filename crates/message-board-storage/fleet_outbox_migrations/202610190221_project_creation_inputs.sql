CREATE TABLE project_creation_inputs (
    request_id TEXT PRIMARY KEY REFERENCES fleet_outbox_requests(request_id),
    canonical_hash TEXT NOT NULL,
    input TEXT NOT NULL
);
