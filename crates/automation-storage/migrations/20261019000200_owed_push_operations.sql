-- Automation commits its state and owed push operation before the file handoff.
CREATE TABLE owed_push_operations (
    operation_id TEXT NOT NULL PRIMARY KEY,
    push_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    owed_at TEXT NOT NULL
) STRICT;
CREATE INDEX owed_push_operations_order ON owed_push_operations(push_id, operation_id);
