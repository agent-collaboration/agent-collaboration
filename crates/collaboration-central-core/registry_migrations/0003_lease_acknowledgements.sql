CREATE TABLE registry_lease_acknowledgements (
    row_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL UNIQUE,
    version TEXT NOT NULL
);
