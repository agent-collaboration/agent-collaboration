-- One row per admitted person request: its fresh request ID (the proof's jti)
-- is the key, so a repeated request ID is refused at insert (r3 §6, §8).
CREATE TABLE organization_used_requests (
    id TEXT NOT NULL PRIMARY KEY,
    organization_id TEXT NOT NULL,
    request_time_ms INTEGER NOT NULL,
    operation_id TEXT,
    actor_person_id TEXT NOT NULL,
    actor_session_id TEXT NOT NULL,
    intent_digest TEXT NOT NULL,
    proof_key_thumbprint TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX organization_used_requests_time
    ON organization_used_requests(request_time_ms);
