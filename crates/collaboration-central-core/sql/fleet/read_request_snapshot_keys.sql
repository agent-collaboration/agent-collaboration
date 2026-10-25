SELECT CAST('request' AS TEXT) AS kind, request_id AS record_id
FROM fleet_request_decisions
WHERE request_id > ?1 OR (request_id = ?1 AND CAST('request' AS TEXT) > ?2)
ORDER BY request_id LIMIT CAST(?3 AS INTEGER)
