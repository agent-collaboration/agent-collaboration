CREATE TABLE legacy_cursor_key (
    singleton INTEGER PRIMARY KEY,
    key_bytes BLOB NOT NULL
);
CREATE TABLE legacy_position_mapping (
    old_position INTEGER PRIMARY KEY,
    project_id TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    position INTEGER NOT NULL
);
CREATE TABLE legacy_cursor_scope_keys (
    scope TEXT PRIMARY KEY
);
CREATE TABLE legacy_cursor_scopes (
    scope TEXT NOT NULL REFERENCES legacy_cursor_scope_keys(scope),
    project_id TEXT NOT NULL,
    PRIMARY KEY(scope, project_id)
);
