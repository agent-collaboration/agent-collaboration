CREATE TABLE workspaces (
    workspace_id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    payload TEXT NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    import_digest TEXT
);
CREATE TABLE boards (
    board_id TEXT NOT NULL PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    name TEXT NOT NULL,
    payload TEXT NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    import_digest TEXT,
    UNIQUE(workspace_id, name),
    UNIQUE(board_id, workspace_id)
);
CREATE TABLE project_directory (
    project_id TEXT NOT NULL PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    board_id TEXT NOT NULL,
    name TEXT NOT NULL,
    payload TEXT NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    import_digest TEXT,
    UNIQUE(board_id, name),
    FOREIGN KEY(board_id, workspace_id) REFERENCES boards(board_id, workspace_id)
);
CREATE TABLE workspace_adrs (
    adr_id TEXT NOT NULL PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    title TEXT NOT NULL,
    payload TEXT NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    import_digest TEXT
);
CREATE TABLE repository_attachments (
    attachment_id TEXT NOT NULL PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id),
    repository_reference TEXT NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    import_digest TEXT,
    UNIQUE(workspace_id, repository_reference)
);
CREATE TABLE fleet_feed (
    entry_id TEXT NOT NULL PRIMARY KEY,
    position INTEGER NOT NULL UNIQUE,
    record_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    as_of TEXT NOT NULL
);
CREATE TABLE fleet_feed_state (
    state_id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    last_position INTEGER NOT NULL,
    retained_through INTEGER NOT NULL
);

CREATE TABLE fleet_request_decisions_with_origin (
    request_id TEXT NOT NULL PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    payload TEXT NOT NULL,
    router_time TEXT NOT NULL,
    time_basis TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    board_id TEXT,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL
);
INSERT INTO fleet_request_decisions_with_origin
    (request_id, canonical_hash, payload, router_time, time_basis, workspace_id,
     board_id, origin_installation, origin_machine)
SELECT request_id, canonical_hash, payload, router_time, time_basis,
       json_extract(payload, '$.envelope.scope.workspace'),
       json_extract(payload, '$.envelope.scope.board'),
       json_extract(payload, '$.envelope.position.position.origin.installation'),
       json_extract(payload, '$.envelope.position.position.origin.machine')
FROM fleet_request_decisions;
DROP TABLE fleet_request_decisions;
ALTER TABLE fleet_request_decisions_with_origin RENAME TO fleet_request_decisions;
