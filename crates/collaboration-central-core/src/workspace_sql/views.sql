WITH request AS (SELECT ?1 AS workspace,?3 AS reserved,?4 AS actor,json_extract(?2,'$.kind') AS op,json_extract(?2,'$.query.kind') AS q,COALESCE(json_extract(?2,'$.after'),0) AS after_position,COALESCE(json_extract(?2,'$.limit'),1)+1 AS page_limit),
candidates(kind,id) AS (SELECT * FROM (
SELECT json_extract(?2,'$.item.kind'),json_extract(?2,'$.item.id') FROM request WHERE op='show'
UNION ALL
SELECT 'area' AS kind,area_id AS id FROM areas,request WHERE op='list' AND (q='areas')
UNION ALL
SELECT 'project' AS kind,project_id AS id FROM projects,request WHERE op='list' AND (q='projects' AND area_id=json_extract(?2,'$.query.areaId'))
UNION ALL
SELECT 'thread' AS kind,thread_id AS id FROM threads,request WHERE op='list' AND (q='threads' AND area_id=json_extract(?2,'$.query.areaId'))
)
UNION ALL
SELECT * FROM (
SELECT 'post' AS kind,post_id AS id FROM posts,request WHERE op='list' AND (q='posts' AND thread_id=json_extract(?2,'$.query.threadId'))
UNION ALL
SELECT 'deliverable' AS kind,deliverable_id AS id FROM deliverables,request WHERE op='list' AND (q='deliverables' AND project_id=json_extract(?2,'$.query.projectId'))
UNION ALL
SELECT 'issue' AS kind,issue_id AS id FROM issues,request WHERE op='list' AND (q='issues' AND deliverable_id=json_extract(?2,'$.query.deliverableId'))
UNION ALL
SELECT 'workLogEntry' AS kind,entry_id AS id FROM work_log_entries,request WHERE op='list' AND (q='workLog' AND project_id=json_extract(?2,'$.query.projectId'))
)
UNION ALL
SELECT * FROM (
SELECT 'backlogItem' AS kind,item_id AS id FROM backlog_items,request WHERE op='list' AND (q='backlog' AND area_id=json_extract(?2,'$.query.areaId'))
UNION ALL
SELECT 'reference' AS kind,reference_id AS id FROM item_references,request WHERE op='list' AND ((q='referencesTo' AND target_kind=json_extract(?2,'$.query.item.kind') AND target_id=json_extract(?2,'$.query.item.id')) OR (q='referencesFrom' AND source_kind=json_extract(?2,'$.query.item.kind') AND source_id=json_extract(?2,'$.query.item.id')))
UNION ALL
SELECT 'link' AS kind,link_id AS id FROM links,request WHERE op='list' AND (q='links' AND source_kind=json_extract(?2,'$.query.item.kind') AND source_id=json_extract(?2,'$.query.item.id'))
UNION ALL
SELECT 'post' AS kind,post_id AS id FROM posts p,request WHERE op='list' AND (q='searchPosts' AND (json_extract(?2,'$.query.threadId') IS NULL OR p.thread_id=json_extract(?2,'$.query.threadId')) AND EXISTS (SELECT 1 FROM threads t WHERE t.thread_id=p.thread_id AND (json_extract(?2,'$.query.areaId') IS NULL OR t.area_id=json_extract(?2,'$.query.areaId'))) AND p.body LIKE '%' || replace(replace(replace(json_extract(?2,'$.query.text'),'\','\\'),'%','\%'),'_','\_') || '%' ESCAPE '\')
)),
positioned AS (SELECT kind,id,(SELECT MIN(position) FROM changes c WHERE c.item_kind=candidates.kind AND c.item_id=candidates.id AND c.change_kind IN ('created','posted')) AS position FROM candidates),
selected_items(kind,id) AS (SELECT kind,id FROM positioned,request WHERE op='show' OR position>after_position ORDER BY position LIMIT (SELECT page_limit FROM request)),
followed_threads(thread_id) AS (SELECT t.thread_id FROM threads t,request WHERE t.creator=actor OR EXISTS(SELECT 1 FROM thread_members m WHERE m.thread_id=t.thread_id AND m.actor_identity_id=actor AND m.left_at IS NULL AND m.notifications=1)),
inbox_changes AS (SELECT c.* FROM changes c,request WHERE op='inbox' AND c.thread_id IN (SELECT thread_id FROM followed_threads) AND c.position>MAX(COALESCE(json_extract(?2,'$.after'),0),COALESCE((SELECT through_position FROM read_marks WHERE actor_identity_id=request.actor AND scope='{"kind":"inbox"}'),0)) ORDER BY c.position LIMIT (SELECT page_limit FROM request)),
view_items(kind,id) AS (SELECT kind,id FROM selected_items UNION SELECT item_kind,item_id FROM inbox_changes),
selected_threads(thread_id) AS (SELECT id FROM view_items WHERE kind='thread' UNION SELECT p.thread_id FROM posts p JOIN view_items v ON v.kind='post' AND v.id=p.post_id UNION SELECT thread_id FROM inbox_changes)
SELECT * FROM (
SELECT 'area' AS row_kind,json_object('areaId',a.area_id,'workspaceId',?1,'name',a.name,'state',a.state) AS payload FROM areas a WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='area' AND v.id=a.area_id)
UNION ALL
SELECT 'project' AS row_kind,json_object('projectId',p.project_id,'areaId',p.area_id,'title',p.title,'status',p.status,'lead',(SELECT holder FROM project_roles r WHERE r.project_id=p.project_id AND r.role='lead' AND r.ended_at IS NULL)) AS payload FROM projects p WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='project' AND v.id=p.project_id)
UNION ALL
SELECT 'initiative' AS row_kind,json_object('initiativeId',n.initiative_id,'name',n.name,'description',n.description,'status',n.status) AS payload FROM initiatives n WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='initiative' AND v.id=n.initiative_id)
UNION ALL
SELECT 'deliverable' AS row_kind,json_object('deliverableId',d.deliverable_id,'projectId',d.project_id,'title',d.title) AS payload FROM deliverables d WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='deliverable' AND v.id=d.deliverable_id)
)
UNION ALL
SELECT * FROM (
SELECT 'dependency' AS row_kind,json_object('dependencyId',e.dependency_id,'blocking',e.blocking_deliverable_id,'blocked',e.blocked_deliverable_id) AS payload FROM deliverable_dependencies e WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='deliverable' AND v.id=e.blocked_deliverable_id)
UNION ALL
SELECT 'issue' AS row_kind,json_object('issueId',i.issue_id,'deliverableId',i.deliverable_id,'title',i.title,'status',i.status) AS payload FROM issues i WHERE EXISTS(SELECT 1 FROM view_items v WHERE (v.kind='issue' AND v.id=i.issue_id) OR (v.kind='deliverable' AND v.id=i.deliverable_id))
UNION ALL
SELECT 'thread' AS row_kind,json_object('threadId',t.thread_id,'areaId',t.area_id,'title',t.title,'creator',t.creator,'state',t.state) AS payload FROM threads t WHERE t.thread_id IN (SELECT thread_id FROM selected_threads)
UNION ALL
SELECT 'member' AS row_kind,json_object('threadMemberId',m.thread_member_id,'threadId',m.thread_id,'actorIdentityId',m.actor_identity_id,'role',m.member_role,'notifications',CASE m.notifications WHEN 1 THEN 'on' WHEN 0 THEN 'off' ELSE NULL END,'state',json(CASE WHEN m.left_at IS NULL THEN '{"kind":"joined"}' ELSE json_object('kind','left','at',m.left_at) END)) AS payload FROM thread_members m WHERE m.thread_id IN (SELECT thread_id FROM selected_threads) AND m.actor_identity_id=?4
)
UNION ALL
SELECT * FROM (
SELECT 'post' AS row_kind,json_object('postId',p.post_id,'threadId',p.thread_id,'author',p.author,'body',p.body,'addressees',p.addressees) AS payload FROM posts p WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='post' AND v.id=p.post_id)
UNION ALL
SELECT 'entry' AS row_kind,json_object('entryId',e.entry_id,'projectId',e.project_id,'author',e.author,'body',json(CASE e.redacted WHEN 1 THEN CASE WHEN e.body IS NULL THEN '{"kind":"redacted"}' ELSE NULL END WHEN 0 THEN json_object('kind','visible','body',e.body) ELSE NULL END),'tags',json((SELECT json_group_array(tag) FROM work_log_tags g WHERE g.entry_id=e.entry_id ORDER BY g.work_log_tag_id))) AS payload FROM work_log_entries e WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='workLogEntry' AND v.id=e.entry_id)
UNION ALL
SELECT 'backlog' AS row_kind,json_object('itemId',b.item_id,'areaId',b.area_id,'title',b.title,'description',b.description,'creator',b.creator,'status',b.status,'closingNote',b.closing_note) AS payload FROM backlog_items b WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='backlogItem' AND v.id=b.item_id)
UNION ALL
SELECT 'reference' AS row_kind,json_object('referenceId',r.reference_id,'source',json(json_object('kind',r.source_kind,'id',r.source_id)),'target',json(json_object('kind',r.target_kind,'id',r.target_id)),'details',r.details) AS payload FROM item_references r WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='reference' AND v.id=r.reference_id)
)
UNION ALL
SELECT * FROM (
SELECT 'link' AS row_kind,json_object('linkId',l.link_id,'source',json(json_object('kind',l.source_kind,'id',l.source_id)),'url',l.url,'title',l.title,'details',l.details) AS payload FROM links l WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='link' AND v.id=l.link_id)
UNION ALL
SELECT 'mark' AS row_kind,json_object('readMarkId',m.read_mark_id,'actorIdentityId',m.actor_identity_id,'scope',m.scope,'throughPosition',m.through_position) AS payload FROM read_marks m WHERE m.actor_identity_id=?4
UNION ALL
SELECT 'change' AS row_kind,json_object('changeId',c.change_id,'position',c.position,'item',json(json_object('kind',c.item_kind,'id',c.item_id)),'actor',c.actor,'changeKind',c.change_kind,'threadId',c.thread_id,'projectId',c.project_id,'subjectActor',c.subject_actor,'role',c.role) AS payload FROM changes c WHERE c.change_id IN (SELECT change_id FROM inbox_changes) OR EXISTS(SELECT 1 FROM view_items v WHERE v.kind=c.item_kind AND v.id=c.item_id AND c.change_kind IN ('created','posted'))
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM changes
);
