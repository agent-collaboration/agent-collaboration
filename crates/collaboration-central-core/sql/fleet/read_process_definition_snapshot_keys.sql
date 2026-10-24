SELECT CAST('processDefinition' AS TEXT) AS kind, definition_id AS record_id
FROM process_definitions
WHERE definition_id > ?1 OR (definition_id = ?1 AND CAST('processDefinition' AS TEXT) > ?2)
ORDER BY definition_id LIMIT CAST(?3 AS INTEGER)
