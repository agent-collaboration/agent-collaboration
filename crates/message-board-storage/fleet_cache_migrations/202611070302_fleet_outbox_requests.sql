-- Moved unchanged from fleet-outbox.sqlite by update step 0005 (design 3 tables page:
-- owner-approved move, columns unchanged). Their keys are the request ids central
-- assigns, which predate the UUIDv7 table rule; they keep them so stored requests,
-- decisions and retries carry the same identities after the move.
CREATE TABLE fleet_outbox_requests (
    request_id TEXT PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    request TEXT NOT NULL,
    state TEXT NOT NULL,
    settled_at_ms INTEGER
);
CREATE INDEX pending_fleet_by_workspace ON fleet_outbox_requests(state, json_extract(request, '$.workspace'), json_extract(request, '$.operation.kind'), request_id);
CREATE INDEX pending_project_boards ON fleet_outbox_requests(state, json_extract(request, '$.operation.kind'), json_extract(request, '$.operation.board'), request_id);
CREATE INDEX fleet_outbox_settled_at ON fleet_outbox_requests(settled_at_ms, request_id);
CREATE TABLE fleet_project_creation_inputs (
    request_id TEXT PRIMARY KEY REFERENCES fleet_outbox_requests(request_id),
    canonical_hash TEXT NOT NULL,
    input TEXT NOT NULL
);
