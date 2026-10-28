WITH sweep_bound AS (SELECT ? AS cut, ? AS project)
SELECT position AS "position!: i64", epoch AS "epoch!: i64", record_id AS "record_id!: String",
 router_time AS "router_time?: String", time_basis AS "time_basis!: String"
FROM project_feed CROSS JOIN sweep_bound sweep
WHERE record_kind='post' AND position>? AND position<=?
 AND (?=0 OR json_extract(content, '$.record.record.placement.kind')='project')
 AND (? IS NULL OR record_id=? OR json_extract(content, '$.record.record.placement.threadId')=?)
 AND (? IS NULL OR instr(lower(json_extract(content, '$.record.record.text')), ?)>0)
 AND (?='both'
  OR (?='topLevel' AND (json_extract(content, '$.record.record.placement.kind')='project' OR record_id=json_extract(content, '$.record.record.placement.threadId')))
  OR (?='thread' AND json_extract(content, '$.record.record.placement.kind')='thread' AND record_id!=json_extract(content, '$.record.record.placement.threadId')))
 AND (sweep.cut IS NULL OR
  (CASE time_basis WHEN 'legacy_unknown' THEN 0 ELSE 1 END, COALESCE(router_time,''), sweep.project, position)
  < (CASE json_extract(sweep.cut, '$.time.kind') WHEN 'unknown' THEN 0 ELSE 1 END, COALESCE(json_extract(sweep.cut, '$.time.value'),''), json_extract(sweep.cut, '$.projectId'), json_extract(sweep.cut, '$.position')))
ORDER BY position DESC LIMIT ?
