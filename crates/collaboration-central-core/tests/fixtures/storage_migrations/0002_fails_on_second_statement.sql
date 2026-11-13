-- Test-only: the first statement succeeds, the second names a table that already exists.
CREATE TABLE widget_labels (label_id TEXT PRIMARY KEY, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE widgets (widget_id TEXT PRIMARY KEY);
