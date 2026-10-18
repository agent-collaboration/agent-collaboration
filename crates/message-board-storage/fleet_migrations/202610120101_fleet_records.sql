CREATE TABLE fleet_records (
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    workspace TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(kind, id)
);
CREATE TABLE fleet_workspaces (
    workspace_id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL DEFAULT 'workspace',
    FOREIGN KEY(kind, workspace_id) REFERENCES fleet_records(kind, id)
);
CREATE TABLE fleet_boards (
    board_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES fleet_workspaces(workspace_id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'board',
    UNIQUE(workspace_id, name),
    FOREIGN KEY(kind, board_id) REFERENCES fleet_records(kind, id)
);
CREATE TABLE fleet_directory (
    project_id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES fleet_boards(board_id),
    title TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'project',
    UNIQUE(board_id, title),
    FOREIGN KEY(kind, project_id) REFERENCES fleet_records(kind, id)
);
CREATE TABLE fleet_definitions (
    workspace_id TEXT NOT NULL REFERENCES fleet_workspaces(workspace_id),
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    record_id TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'processDefinition',
    PRIMARY KEY(workspace_id, name, version),
    FOREIGN KEY(kind, record_id) REFERENCES fleet_records(kind, id)
);
CREATE TABLE fleet_repositories (
    workspace_id TEXT NOT NULL REFERENCES fleet_workspaces(workspace_id),
    reference TEXT NOT NULL,
    PRIMARY KEY(workspace_id, reference)
);
CREATE TABLE archive_reports (
    board TEXT NOT NULL REFERENCES fleet_boards(board_id),
    project TEXT NOT NULL REFERENCES fleet_directory(project_id),
    position_epoch TEXT NOT NULL,
    position_commit TEXT NOT NULL,
    installation TEXT NOT NULL,
    reported_at TEXT NOT NULL,
    PRIMARY KEY(board, project)
);
CREATE TABLE fleet_request_ledger (
    request_id TEXT PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    payload TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'request',
    FOREIGN KEY(kind, request_id) REFERENCES fleet_records(kind, id)
);
CREATE TABLE fleet_checkpoint (
    singleton INTEGER PRIMARY KEY,
    last_position INTEGER NOT NULL
);
INSERT INTO fleet_checkpoint(singleton, last_position) VALUES (1, 0);
CREATE TABLE fleet_feed (
    position INTEGER PRIMARY KEY,
    entry TEXT NOT NULL
);
