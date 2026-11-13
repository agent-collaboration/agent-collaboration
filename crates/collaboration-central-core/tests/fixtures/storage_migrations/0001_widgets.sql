-- Test-only schema for the storage boundary's O1 cases; no product table.
CREATE TABLE widget_owners (owner_id TEXT PRIMARY KEY, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE widgets (widget_id TEXT PRIMARY KEY, owner_id TEXT NOT NULL REFERENCES widget_owners(owner_id), name TEXT NOT NULL, active INTEGER NOT NULL CHECK (active IN (0, 1)), retired_at TEXT, due_at_ms INTEGER, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX widgets_live_name ON widgets(name) WHERE retired_at IS NULL;
