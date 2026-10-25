SELECT DISTINCT r.row_id AS "row_id!: String", json_object('rowId',r.row_id,'id',r.row_id,'project',p.project_id,'scope',p.project_id,
 'position',json_object('kind','project','position',json_object('cursor',json_object('epoch',r.epoch,'position',r.position),'origin',json_object('installation',f.origin_installation,'machine',f.origin_machine))),
 'actorId',r.actor_id,'actor',json(a.canonical_identity),'subject',json(r.subject),
 'tags',json((SELECT json_group_array(tag) FROM (SELECT tag FROM work_trail_tags tags WHERE tags.row_id=r.row_id ORDER BY place))),
 'links',json((SELECT json_group_array(json(target)) FROM (SELECT target FROM work_trail_links links WHERE links.row_id=r.row_id ORDER BY place)))) AS "metadata!: String"
FROM work_trail_tags t CROSS JOIN work_trail_rows r ON r.row_id=t.row_id
CROSS JOIN project_state p
LEFT JOIN actors a ON a.actor_id=r.actor_id
LEFT JOIN project_feed f ON f.epoch=r.epoch AND f.position=r.position
WHERE t.tag=? AND r.row_id>? AND r.redacted=0
ORDER BY r.row_id LIMIT 101
