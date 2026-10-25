SELECT CAST('project' AS TEXT) AS kind, project_id AS record_id
FROM project_directory
WHERE project_id > ?1 OR (project_id = ?1 AND CAST('project' AS TEXT) > ?2)
ORDER BY project_id LIMIT CAST(?3 AS INTEGER)
