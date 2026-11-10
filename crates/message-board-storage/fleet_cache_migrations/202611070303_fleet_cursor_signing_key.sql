-- Moved unchanged from board-cursors.sqlite by update step 0005 (design 3 tables page:
-- owner-approved rename, columns unchanged). key_id is already a UUIDv7; the one
-- signing key has no created_at/updated_at because it is never rewritten.
CREATE TABLE fleet_cursor_signing_key (
    key_id TEXT PRIMARY KEY,
    key_bytes BLOB NOT NULL
);
