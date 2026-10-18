WITH selection AS (SELECT ? AS subject), selected AS (SELECT json_extract(subject, '$.kind') AS kind, json_extract(subject, '$.subject') AS id FROM selection)
SELECT DISTINCT t.row_id AS "row_id!: String", json_object('rowId', json_extract(r.payload, '$.record.rowId'), 'id', json_extract(r.payload, '$.record.envelope.id'), 'project', json_extract(r.payload, '$.record.projectId'), 'scope', json_extract(r.payload, '$.record.envelope.scope.project'), 'position', json_extract(r.payload, '$.record.envelope.position'), 'actor', json_extract(r.payload, '$.record.envelope.actor.actor'), 'subject', json_extract(r.payload, '$.record.subject'), 'tags', json_extract(r.payload, '$.record.tags'), 'links', json_extract(r.payload, '$.record.links')) AS "metadata!: String"
FROM work_trail_tags t INDEXED BY trail_by_tag CROSS JOIN work_trail_rows current ON current.row_id = t.row_id
CROSS JOIN record_identities r INDEXED BY sqlite_autoindex_record_identities_1 ON r.kind = 'workTrailRow' AND r.id = CAST(t.row_id AS TEXT)
CROSS JOIN selected s
WHERE t.tag = ? AND t.row_id > ? AND current.redacted = 0
AND (t.tag = 'owner-question' OR json_extract(r.payload, '$.record.envelope.actor.actor.identity') = ?)
AND (s.kind = 'project'
 OR json_extract(r.payload, '$.record.subject.kind') = s.kind AND json_extract(r.payload, '$.record.subject.subject') = s.id
 OR s.kind = 'workstream' AND json_extract(r.payload, '$.record.subject.kind') = 'task' AND EXISTS(SELECT 1 FROM tasks task WHERE task.task_id = json_extract(r.payload, '$.record.subject.subject') AND task.workstream_id = s.id))
ORDER BY t.row_id LIMIT 101
