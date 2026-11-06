CREATE TABLE fleet_creation_decisions (
    project_id TEXT NOT NULL PRIMARY KEY,
    creating_request TEXT NOT NULL UNIQUE REFERENCES fleet_request_decisions(request_id),
    carrier_installation TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
