-- Central-private: which organization's Fleet this Copy reports to, so a
-- restart finds it. Never replicated out of central; grants nothing.
CREATE TABLE _copy_fleet_reference (
    project_id TEXT NOT NULL PRIMARY KEY,
    fleet_address_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
