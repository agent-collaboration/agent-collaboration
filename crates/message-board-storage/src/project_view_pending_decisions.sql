SELECT DISTINCT t.row_id AS "row_id!: String", json_object('rowId', json_extract(r.payload, '$.record.rowId'), 'id', json_extract(r.payload, '$.record.envelope.id'), 'project', json_extract(r.payload, '$.record.projectId'), 'scope', json_extract(r.payload, '$.record.envelope.scope.project'), 'position', json_extract(r.payload, '$.record.envelope.position'), 'actor', json_extract(r.payload, '$.record.envelope.actor.actor'), 'subject', json_extract(r.payload, '$.record.subject'), 'tags', json_extract(r.payload, '$.record.tags'), 'links', json_extract(r.payload, '$.record.links')) AS "metadata!: String",
 CASE WHEN ? = 1 THEN (SELECT json_object('role', json_extract(f.entry, '$.content.record.record.role'), 'subject', json_extract(f.entry, '$.content.record.record.subject'), 'holder', json_extract(f.entry, '$.content.record.record.holder.actor'), 'ended', json_extract(f.entry, '$.content.record.record.ended')) FROM project_feed f
  WHERE f.record_kind = 'roleAssignment' AND f.record_id IN (SELECT assignment_id FROM role_assignments WHERE project_subject = ?)
  AND f.position <= json_extract(r.payload, '$.record.envelope.position.position.position')
  ORDER BY f.position DESC LIMIT 1) END AS "authority: String"
FROM work_trail_tags t CROSS JOIN work_trail_rows current ON current.row_id = t.row_id
CROSS JOIN record_identities r INDEXED BY sqlite_autoindex_record_identities_1 ON r.kind = 'workTrailRow' AND r.id = CAST(t.row_id AS TEXT)
WHERE t.tag = 'decision' AND t.row_id > ? AND current.redacted = 0
AND EXISTS (SELECT 1 FROM json_each(json_extract(r.payload, '$.record.links')) link CROSS JOIN json_each(?) candidate
 WHERE json_extract(link.value, '$.kind') = 'record' AND json_extract(link.value, '$.record.kind') = 'workTrailRow' AND json_extract(link.value, '$.record.id') = CAST(candidate.value AS TEXT))
ORDER BY t.row_id LIMIT 101
