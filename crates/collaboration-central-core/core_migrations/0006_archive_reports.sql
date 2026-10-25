CREATE TABLE archive_reports (
    report_id TEXT NOT NULL PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES boards(board_id),
    project_id TEXT NOT NULL REFERENCES project_directory(project_id),
    report TEXT NOT NULL,
    UNIQUE(board_id, project_id)
);
