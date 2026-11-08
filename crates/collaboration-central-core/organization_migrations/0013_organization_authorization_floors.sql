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
