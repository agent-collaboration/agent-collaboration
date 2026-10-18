SELECT r.id AS "id!: String" FROM record_identities r
WHERE r.kind = ? AND r.id > ?
 AND (r.kind != 'subtask' OR ? = 1 OR EXISTS(SELECT 1 FROM subtasks s WHERE s.subtask_id = r.id))
 AND (r.kind != 'roleAssignment' OR EXISTS(SELECT 1 FROM role_assignments a WHERE a.assignment_id = r.id AND a.active = 1))
 AND (r.kind != 'move' OR json_extract(r.payload, '$.record.envelope.position.position.position') >= COALESCE((SELECT MAX(json_extract(m.payload, '$.record.envelope.position.position.position')) FROM record_identities m WHERE m.kind = 'move' AND json_extract(m.payload, '$.record.outcome.kind') = 'accepted'),0))
 AND (r.kind != 'fact' OR (json_extract(r.payload, '$.record.kind.kind') = 'lifecycle' AND json_extract(r.payload, '$.record.kind.fact.kind') = 'rejectedTail'))
ORDER BY r.id LIMIT 101
