CREATE TABLE fleet_archive_reports (
    report_id TEXT NOT NULL PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES fleet_boards(board_id),
    project_id TEXT NOT NULL REFERENCES project_directory(project_id),
    epoch TEXT NOT NULL,
    commit_sequence TEXT NOT NULL,
    installation TEXT NOT NULL,
    reported_at TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(board_id, project_id)
);
