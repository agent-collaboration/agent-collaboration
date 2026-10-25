SELECT CAST('project' AS TEXT) AS kind, project_id AS record_id
FROM project_directory
WHERE (?1 IS NULL OR project_id > ?1)
ORDER BY project_id LIMIT CAST(?2 AS INTEGER);
