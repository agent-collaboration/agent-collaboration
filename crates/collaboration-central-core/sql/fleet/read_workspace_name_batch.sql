SELECT CAST(workspace_id AS TEXT) AS record_id, CAST('' AS TEXT) AS scope_id, CAST(name AS TEXT) AS name
FROM fleet_workspaces
WHERE name IN (SELECT json_extract(value, '$[1]') FROM json_each(?));
