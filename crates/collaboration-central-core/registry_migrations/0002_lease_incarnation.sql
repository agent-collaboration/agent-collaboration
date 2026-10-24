CREATE TABLE registry_lease_incarnation (
    row_id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    incarnation TEXT NOT NULL
);
