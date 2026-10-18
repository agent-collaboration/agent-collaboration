CREATE TABLE IF NOT EXISTS _registry_lease_run (singleton INTEGER PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS _registry_lease_publications (project TEXT PRIMARY KEY, call_id TEXT UNIQUE, record TEXT NOT NULL);
