SELECT CAST(expires_at AS TEXT) AS expires_at FROM machine_control_project_notices WHERE machine_id=? ORDER BY position LIMIT 1;
