SELECT DISTINCT t.row_id AS "row_id!: String", json_object('rowId', json_extract(r.payload, '$.record.rowId'), 'id', json_extract(r.payload, '$.record.envelope.id'), 'project', json_extract(r.payload, '$.record.projectId'), 'scope', json_extract(r.payload, '$.record.envelope.scope.project'), 'position', json_extract(r.payload, '$.record.envelope.position'), 'actor', json_extract(r.payload, '$.record.envelope.actor.actor'), 'subject', json_extract(r.payload, '$.record.subject'), 'tags', json_extract(r.payload, '$.record.tags'), 'links', json_extract(r.payload, '$.record.links')) AS "metadata!: String"
FROM work_trail_tags t CROSS JOIN work_trail_rows current ON current.row_id = t.row_id
CROSS JOIN record_identities r INDEXED BY sqlite_autoindex_record_identities_1 ON r.kind = 'workTrailRow' AND r.id = CAST(t.row_id AS TEXT)
WHERE t.tag = ? AND t.row_id > ? AND current.redacted = 0
ORDER BY t.row_id LIMIT 101
