CREATE TABLE board_intent_scan_cursors (
    project_id TEXT NOT NULL PRIMARY KEY,
    epoch INTEGER NOT NULL,
    position INTEGER NOT NULL
);
