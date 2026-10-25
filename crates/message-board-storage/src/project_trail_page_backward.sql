WITH filter AS (SELECT ? AS subject, ? AS tags, ? AS include_redacted, ? AS decisions_only)
SELECT f.position AS "position!: i64", f.epoch AS "epoch!: i64", f.record_id AS "id!: String", f.router_time AS "router_time?: String", f.time_basis AS "time_basis!: String"
FROM project_feed f CROSS JOIN filter i
JOIN work_trail_rows current ON current.row_id = f.record_id
WHERE f.record_kind = 'workTrailRow' AND f.position > ? AND f.position <= ?
 AND (i.include_redacted = 1 OR current.redacted = 0)
 AND (i.subject IS NULL OR json_extract(f.content, '$.record.record.subject.kind') = json_extract(i.subject, '$.kind')
  AND json_extract(f.content, '$.record.record.subject.subject') IS json_extract(i.subject, '$.subject'))
 AND (i.decisions_only = 0 OR EXISTS(SELECT 1 FROM work_trail_tags t WHERE t.tag = 'decision' AND t.row_id = f.record_id))
 AND NOT EXISTS(SELECT 1 FROM json_each(i.tags) requested WHERE NOT EXISTS(SELECT 1 FROM work_trail_tags t WHERE t.tag = CAST(requested.value AS TEXT) AND t.row_id = f.record_id))
ORDER BY f.position DESC LIMIT ?
