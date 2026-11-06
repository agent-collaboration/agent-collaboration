DELETE FROM fleet_workspace_repository_refs WHERE workspace_id = ? AND ordinal >= CAST(? AS INTEGER);
