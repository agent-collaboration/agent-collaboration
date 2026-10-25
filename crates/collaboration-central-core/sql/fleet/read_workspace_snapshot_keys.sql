SELECT CAST('workspace' AS TEXT) AS kind, workspace_id AS record_id
FROM workspaces
WHERE workspace_id > ?1 OR (workspace_id = ?1 AND CAST('workspace' AS TEXT) > ?2)
ORDER BY workspace_id LIMIT CAST(?3 AS INTEGER)
