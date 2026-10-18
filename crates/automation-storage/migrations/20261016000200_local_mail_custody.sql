-- The Local identity survives its carrier history. No age-based mail deletion.
CREATE TABLE mail_items (
    item_id TEXT NOT NULL PRIMARY KEY,
    digest TEXT NOT NULL,
    direction TEXT NOT NULL,
    item_json TEXT,
    status_json TEXT NOT NULL,
    status_version INTEGER NOT NULL,
    payload_bytes INTEGER NOT NULL,
    projection_json TEXT,
    result_json TEXT,
    submit_attempted_at TEXT,
    created_at TEXT NOT NULL,
    terminal_at TEXT
) STRICT;
CREATE INDEX mail_items_terminal ON mail_items(terminal_at, item_id);
-- Existing, non-mail carriers retain C5 R7 explicitly.
ALTER TABLE router_pushes ADD COLUMN retention TEXT NOT NULL DEFAULT 'creationAge';
CREATE INDEX router_pushes_mail_retention ON router_pushes(retention, settled_at, push_id);
