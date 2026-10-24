SELECT import_id, project_id, digest, header, phase
FROM admin_imports
WHERE project_id = ? AND digest = ?;
