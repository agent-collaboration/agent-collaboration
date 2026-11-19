-- Sign-in authority tables ported from B2 0012–0015; identities remain in 0002.
-- Who changed what, written in the same transaction as each change.
CREATE TABLE organization_change_records (
    id TEXT NOT NULL PRIMARY KEY,
    request_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    origin TEXT NOT NULL,
    actor_person_id TEXT,
    permission_checked TEXT,
    target_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- The latest organization fence central has received (r3 §6). Its id is the
-- organization's ID. State membership and revision ranges are enforced by the
-- Rust codec at every write and read, not by CHECK constraints (AGENTS.md).
CREATE TABLE organization_authorization_floors (
    id TEXT NOT NULL PRIMARY KEY,
    minimum_revision INTEGER NOT NULL,
    state TEXT NOT NULL,
    permission_policy_digest TEXT NOT NULL,
    request_reject_through_ms INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- The latest person-within-organization fence central has received (r3 §6).
-- Its id is the sign-in service's retained subject UUIDv7 for the pair.
CREATE TABLE person_organization_authorization_floors (
    id TEXT NOT NULL PRIMARY KEY,
    organization_id TEXT NOT NULL,
    person_id TEXT NOT NULL,
    minimum_revision INTEGER NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

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

CREATE UNIQUE INDEX organization_change_records_request ON organization_change_records(request_id);
CREATE UNIQUE INDEX person_organization_authorization_pair ON person_organization_authorization_floors(organization_id,person_id);
