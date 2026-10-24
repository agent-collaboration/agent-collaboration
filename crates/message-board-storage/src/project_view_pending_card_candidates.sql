WITH selected AS (SELECT json_extract(value,'$.kind') AS kind,json_extract(value,'$.subject') AS id FROM (SELECT ? AS value))
SELECT DISTINCT r.row_id AS "row_id!: String", json_object('rowId',r.row_id,'id',r.row_id,'project',p.project_id,'scope',p.project_id,
 'position',json_object('kind','project','position',json_object('cursor',json_object('epoch',r.epoch,'position',r.position),'origin',json_object('installation',f.origin_installation,'machine',f.origin_machine))),
 'actorId',r.actor_id,'actor',json(a.canonical_identity),'subject',json(r.subject),
 'tags',json((SELECT json_group_array(tag) FROM (SELECT tag FROM work_trail_tags tags WHERE tags.row_id=r.row_id ORDER BY place))),
 'links',json((SELECT json_group_array(json(target)) FROM (SELECT target FROM work_trail_links links WHERE links.row_id=r.row_id ORDER BY place)))) AS "metadata!: String"
FROM work_trail_tags t CROSS JOIN work_trail_rows r ON r.row_id=t.row_id
CROSS JOIN project_state p
LEFT JOIN actors a ON a.actor_id=r.actor_id
LEFT JOIN project_feed f ON f.epoch=r.epoch AND f.position=r.position
CROSS JOIN selected s
WHERE t.tag=? AND r.row_id>? AND r.redacted=0 AND (t.tag='owner-question' OR a.canonical_identity=?)
AND (s.kind='project' OR json_extract(r.subject,'$.kind')=s.kind AND json_extract(r.subject,'$.subject')=s.id
 OR s.kind='workstream' AND json_extract(r.subject,'$.kind')='task' AND EXISTS(SELECT 1 FROM tasks task WHERE task.task_id=json_extract(r.subject,'$.subject') AND task.workstream_id=s.id))
ORDER BY r.row_id LIMIT 101
