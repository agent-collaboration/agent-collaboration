SELECT CAST(board_id AS TEXT) AS record_id, CAST(workspace_id AS TEXT) AS scope_id, CAST(name AS TEXT) AS name
FROM fleet_boards
WHERE (workspace_id, name) IN (SELECT json_extract(value, '$[0]'), json_extract(value, '$[1]') FROM json_each(?));
