SELECT CAST('board' AS TEXT) AS kind, board_id AS record_id
FROM boards
WHERE board_id > ?1 OR (board_id = ?1 AND CAST('board' AS TEXT) > ?2)
ORDER BY board_id LIMIT CAST(?3 AS INTEGER)
