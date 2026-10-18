WITH selection AS (SELECT ? AS subject), selected AS (SELECT json_extract(subject, '$.kind') AS kind, json_extract(subject, '$.subject') AS id FROM selection)
SELECT r.id AS "id!: String" FROM record_identities r INDEXED BY trail_current_position CROSS JOIN selected s
JOIN work_trail_rows t ON t.row_id = r.id AND t.redacted = 0
WHERE r.kind = 'workTrailRow' AND (s.kind = 'project'
 OR json_extract(r.payload, '$.record.subject.kind') = s.kind AND json_extract(r.payload, '$.record.subject.subject') = s.id
 OR s.kind = 'workstream' AND json_extract(r.payload, '$.record.subject.kind') = 'task' AND EXISTS(SELECT 1 FROM tasks task WHERE task.task_id = json_extract(r.payload, '$.record.subject.subject') AND task.workstream_id = s.id))
ORDER BY json_extract(r.payload, '$.record.envelope.position.position.position') DESC, r.id DESC LIMIT 21
