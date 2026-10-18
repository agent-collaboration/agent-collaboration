WITH sweep_bound AS (SELECT ? AS cut, ? AS project)
SELECT position AS "position!: i64", epoch AS "epoch!: i64", record_id AS "record_id!: String",
 json_extract(entry, '$.routerTime') AS "router_time!: String"
FROM project_feed CROSS JOIN sweep_bound sweep
WHERE record_kind = 'post' AND position > ? AND position <= ?
 AND (? = 0 OR json_extract(entry, '$.content.record.record.placement.kind') = 'project')
 AND (? IS NULL OR record_id = ? OR json_extract(entry, '$.content.record.record.placement.threadId') = ?)
 AND (? IS NULL OR instr(lower(json_extract(entry, '$.content.record.record.text')), ?) > 0)
 AND (? = 'both'
  OR (? = 'topLevel' AND (json_extract(entry, '$.content.record.record.placement.kind') = 'project' OR record_id = json_extract(entry, '$.content.record.record.placement.threadId')))
  OR (? = 'thread' AND json_extract(entry, '$.content.record.record.placement.kind') = 'thread' AND record_id != json_extract(entry, '$.content.record.record.placement.threadId')))
 AND (sweep.cut IS NULL OR (rtrim(json_extract(entry, '$.routerTime'), 'Z'), sweep.project, position)
  > (rtrim(json_extract(sweep.cut, '$.routerTime'), 'Z'), json_extract(sweep.cut, '$.projectId'), json_extract(sweep.cut, '$.position')))
ORDER BY position ASC
LIMIT ?
