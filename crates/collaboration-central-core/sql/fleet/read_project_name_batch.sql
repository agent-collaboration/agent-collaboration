SELECT CAST(project_id AS TEXT) AS record_id, CAST(board_id AS TEXT) AS scope_id, CAST(name AS TEXT) AS name
FROM fleet_project_directory
WHERE (board_id, name) IN (SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]') FROM json_each(?));
