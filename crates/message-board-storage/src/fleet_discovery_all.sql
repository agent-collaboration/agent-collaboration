SELECT r.kind AS "kind!: String", r.id AS "id!: String",
       json_extract(r.payload, '$.record.envelope.scope.workspace') AS "workspace!: String",
       json_extract(r.payload, '$.record.envelope.scope.board') AS "board: String"
FROM cached_fleet_records r INDEXED BY sqlite_autoindex_cached_fleet_records_1
CROSS JOIN cached_fleet_records w INDEXED BY sqlite_autoindex_cached_fleet_records_1
  ON w.kind = 'workspace' AND w.id = json_extract(r.payload, '$.record.envelope.scope.workspace')
LEFT JOIN cached_fleet_records b INDEXED BY sqlite_autoindex_cached_fleet_records_1
  ON b.kind = 'board' AND b.id = json_extract(r.payload, '$.record.envelope.scope.board')
WHERE r.kind = ?1 AND r.id > ?2
  AND (instr(lower(COALESCE(json_extract(r.payload, '$.record.name'), json_extract(r.payload, '$.record.title'), '')), ?3) > 0
       OR (r.kind <> 'project' AND instr(lower(COALESCE(json_extract(r.payload, '$.record.description'), '')), ?3) > 0))
  AND (r.kind <> 'project' OR b.id IS NOT NULL)
  AND (?4 OR (json_extract(w.payload, '$.record.state') = 'active'
       AND (r.kind = 'workspace' OR json_extract(b.payload, '$.record.state') IN ('open', 'closed'))))
ORDER BY r.id LIMIT ?5
