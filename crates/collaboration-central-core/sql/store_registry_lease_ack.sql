INSERT INTO registry_lease_acknowledgements(row_id, project_id, version) VALUES (?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET version = excluded.version;
