-- Test-support only. The host's fixture migrator appends this after production
-- migrations; no production database receives these machine subjects.
CREATE TABLE engine_probe_groups (
    group_id TEXT PRIMARY KEY,
    effect_count INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE engine_probe_items (
    item_id TEXT PRIMARY KEY,
    machine_kind TEXT NOT NULL,
    group_id TEXT NOT NULL REFERENCES engine_probe_groups(group_id),
    due_at TEXT,
    claim TEXT NOT NULL,
    state TEXT NOT NULL,
    completed_claim TEXT,
    send_id TEXT,
    connection_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX engine_probe_completed_claim ON engine_probe_items(completed_claim);
