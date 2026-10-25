SELECT CAST('adr' AS TEXT) AS kind, adr_id AS record_id
FROM workspace_adrs
WHERE adr_id > ?1 OR (adr_id = ?1 AND CAST('adr' AS TEXT) > ?2)
ORDER BY adr_id LIMIT CAST(?3 AS INTEGER)
