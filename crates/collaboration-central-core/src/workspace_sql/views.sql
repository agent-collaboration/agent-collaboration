WITH request AS (SELECT ?1 AS workspace,?3 AS reserved,?4 AS actor,json_extract(?2,'$.kind') AS op,json_extract(?2,'$.query.kind') AS q,json_extract(?2,'$.cursor.order') AS list_order,json_extract(?2,'$.cursor.position') AS cursor_position,json_extract(?2,'$.after') AS inbox_after,json_extract(?2,'$.query.scope.kind') AS post_scope,'%' || replace(replace(replace(json_extract(?2,'$.query.text'),'\','\\'),'%','\%'),'_','\_') || '%' AS text_pattern,COALESCE(json_extract(?2,'$.limit'),1)+1 AS page_limit),
candidates(kind,id) AS (SELECT * FROM (
SELECT json_extract(?2,'$.item.kind'),json_extract(?2,'$.item.id') FROM request WHERE op='show'
UNION ALL
SELECT 'area',area_id FROM workspace_areas,request WHERE op='list' AND ((q='areas' AND (json_extract(?2,'$.query.includeArchived')=1 OR state='open')) OR (q='searchItems' AND json_extract(?2,'$.query.itemKind') IN ('all','area') AND (json_extract(?2,'$.query.includeArchived')=1 OR state='open') AND (name LIKE text_pattern ESCAPE '\' OR description LIKE text_pattern ESCAPE '\')))
UNION ALL
SELECT 'initiative',initiative_id FROM workspace_initiatives,request WHERE op='list' AND (q='initiatives' OR (q='searchItems' AND json_extract(?2,'$.query.itemKind') IN ('all','initiative') AND (name LIKE text_pattern ESCAPE '\' OR description LIKE text_pattern ESCAPE '\')))
UNION ALL
SELECT 'project',project_id FROM workspace_projects,request WHERE op='list' AND ((q='projects' AND area_id=json_extract(?2,'$.query.areaId')) OR (q='searchItems' AND json_extract(?2,'$.query.itemKind') IN ('all','project') AND (title LIKE text_pattern ESCAPE '\' OR description LIKE text_pattern ESCAPE '\')))
)
UNION ALL
SELECT * FROM (
SELECT 'thread',t.thread_id FROM workspace_threads t,request WHERE op='list' AND q='threads' AND (json_extract(?2,'$.query.areaId') IS NULL OR t.area_id=json_extract(?2,'$.query.areaId')) AND (json_extract(?2,'$.query.followedOnly')=0 OR EXISTS(SELECT 1 FROM workspace_thread_members m WHERE m.thread_id=t.thread_id AND m.actor_identity_id=request.actor AND m.left_at IS NULL))
UNION ALL
SELECT 'post',p.post_id FROM workspace_posts p,request WHERE op='list' AND q IN ('posts','searchPosts') AND (post_scope='workspace' OR (post_scope='thread' AND p.thread_id=json_extract(?2,'$.query.scope.threadId')) OR (post_scope='area' AND EXISTS(SELECT 1 FROM workspace_threads t WHERE t.thread_id=p.thread_id AND t.area_id=json_extract(?2,'$.query.scope.areaId')))) AND (q='posts' OR p.body LIKE text_pattern ESCAPE '\')
UNION ALL
SELECT 'deliverable',deliverable_id FROM workspace_deliverables,request WHERE op='list' AND q='deliverables' AND project_id=json_extract(?2,'$.query.projectId')
UNION ALL
SELECT 'issue',i.issue_id FROM workspace_issues i,request WHERE op='list' AND ((q='issues' AND i.deliverable_id=json_extract(?2,'$.query.deliverableId')) OR (q='projectIssues' AND EXISTS(SELECT 1 FROM workspace_deliverables d WHERE d.deliverable_id=i.deliverable_id AND d.project_id=json_extract(?2,'$.query.projectId'))))
)
UNION ALL
SELECT * FROM (
SELECT 'workLogEntry',e.entry_id FROM workspace_work_log_entries e,request WHERE op='list' AND q='workLog' AND e.project_id=json_extract(?2,'$.query.projectId') AND NOT EXISTS(SELECT 1 FROM json_each(json_extract(?2,'$.query.tags')) wanted WHERE NOT EXISTS(SELECT 1 FROM workspace_work_log_tags g WHERE g.entry_id=e.entry_id AND g.tag=CAST(wanted.value AS TEXT)))
UNION ALL
SELECT 'backlogItem',item_id FROM workspace_backlog_items,request WHERE op='list' AND q='backlog' AND area_id=json_extract(?2,'$.query.areaId')
UNION ALL
SELECT 'reference',reference_id FROM workspace_item_references,request WHERE op='list' AND ((q='referencesTo' AND target_kind=json_extract(?2,'$.query.item.kind') AND target_id=json_extract(?2,'$.query.item.id')) OR (q='referencesFrom' AND source_kind=json_extract(?2,'$.query.item.kind') AND source_id=json_extract(?2,'$.query.item.id')))
UNION ALL
SELECT 'link',link_id FROM workspace_links,request WHERE op='list' AND q='links' AND source_kind=json_extract(?2,'$.query.item.kind') AND source_id=json_extract(?2,'$.query.item.id')
)),
-- Memberships and role assignments have no creating change; their stable
-- position is their ordinal among all rows of their table by UUIDv7 id.
ordinal_candidates(kind,id,position) AS (SELECT * FROM (
SELECT 'threadMember',m.thread_member_id,(SELECT COUNT(*) FROM workspace_thread_members o WHERE o.thread_member_id<=m.thread_member_id) FROM workspace_thread_members m,request WHERE (op='show' AND json_extract(?2,'$.item.kind')='threadMember' AND m.thread_member_id=json_extract(?2,'$.item.id')) OR (op='list' AND q='threadMembers' AND m.thread_id=json_extract(?2,'$.query.threadId') AND (json_extract(?2,'$.query.includeLeft')=1 OR m.left_at IS NULL))
UNION ALL
SELECT 'projectRole',r.project_role_id,(SELECT COUNT(*) FROM workspace_project_roles o WHERE o.project_role_id<=r.project_role_id) FROM workspace_project_roles r,request WHERE (op='show' AND json_extract(?2,'$.item.kind')='projectRole' AND r.project_role_id=json_extract(?2,'$.item.id')) OR (op='list' AND q='projectRoles' AND r.project_id=json_extract(?2,'$.query.projectId') AND (json_extract(?2,'$.query.includeEnded')=1 OR r.ended_at IS NULL))
UNION ALL
SELECT 'deliverableRole',r.deliverable_role_id,(SELECT COUNT(*) FROM workspace_deliverable_roles o WHERE o.deliverable_role_id<=r.deliverable_role_id) FROM workspace_deliverable_roles r,request WHERE (op='show' AND json_extract(?2,'$.item.kind')='deliverableRole' AND r.deliverable_role_id=json_extract(?2,'$.item.id')) OR (op='list' AND q='deliverableRoles' AND r.deliverable_id=json_extract(?2,'$.query.deliverableId') AND (json_extract(?2,'$.query.includeEnded')=1 OR r.ended_at IS NULL))
)),
positioned(kind,id,position) AS (SELECT c.kind,c.id,(SELECT MIN(position) FROM workspace_changes x WHERE x.item_kind=c.kind AND x.item_id=c.id AND x.change_kind IN ('created','posted')) FROM candidates c WHERE c.kind NOT IN ('threadMember','projectRole','deliverableRole') UNION ALL SELECT kind,id,position FROM ordinal_candidates),
selected_items(kind,id,position) AS (SELECT kind,id,position FROM positioned,request WHERE op='show' OR cursor_position IS NULL OR (list_order='newestFirst' AND position<cursor_position) OR (list_order='oldestFirst' AND position>cursor_position) ORDER BY CASE WHEN list_order='newestFirst' THEN -position ELSE position END LIMIT (SELECT page_limit FROM request)),
followed_threads(thread_id) AS (SELECT t.thread_id FROM workspace_threads t,request WHERE t.creator=actor OR EXISTS(SELECT 1 FROM workspace_thread_members m WHERE m.thread_id=t.thread_id AND m.actor_identity_id=actor AND m.left_at IS NULL AND m.notifications=1)),
inbox_changes AS (SELECT c.* FROM workspace_changes c,request WHERE op='inbox' AND c.thread_id IN (SELECT thread_id FROM followed_threads) AND c.position>MAX(COALESCE(inbox_after,0),COALESCE((SELECT through_position FROM workspace_read_marks WHERE actor_identity_id=request.actor AND scope='{"kind":"inbox"}'),0)) ORDER BY c.position LIMIT (SELECT page_limit FROM request)),
view_items(kind,id) AS (SELECT kind,id FROM selected_items UNION SELECT item_kind,item_id FROM inbox_changes),
selected_threads(thread_id) AS (SELECT id FROM view_items WHERE kind='thread' UNION SELECT p.thread_id FROM workspace_posts p JOIN view_items v ON v.kind='post' AND v.id=p.post_id UNION SELECT thread_id FROM inbox_changes WHERE thread_id IS NOT NULL UNION SELECT m.thread_id FROM workspace_thread_members m JOIN view_items v ON v.kind='threadMember' AND v.id=m.thread_member_id)
SELECT * FROM (
SELECT 'area' AS row_kind,json_object('areaId',a.area_id,'workspaceId',?1,'name',a.name,'description',a.description,'state',a.state) AS payload FROM workspace_areas a WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='area' AND v.id=a.area_id)
UNION ALL
SELECT 'project' AS row_kind,json_object('projectId',p.project_id,'areaId',p.area_id,'title',p.title,'description',p.description,'status',p.status,'lead',(SELECT holder FROM workspace_project_roles r WHERE r.project_id=p.project_id AND r.role='lead' AND r.ended_at IS NULL)) AS payload FROM workspace_projects p WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='project' AND v.id=p.project_id)
UNION ALL
SELECT 'initiative' AS row_kind,json_object('initiativeId',n.initiative_id,'name',n.name,'description',n.description,'status',n.status) AS payload FROM workspace_initiatives n WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='initiative' AND v.id=n.initiative_id)
UNION ALL
SELECT 'deliverable' AS row_kind,json_object('deliverableId',d.deliverable_id,'projectId',d.project_id,'title',d.title) AS payload FROM workspace_deliverables d,request WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='deliverable' AND v.id=d.deliverable_id) OR EXISTS(SELECT 1 FROM view_items v JOIN workspace_issues i ON v.kind='issue' AND v.id=i.issue_id WHERE i.deliverable_id=d.deliverable_id AND request.q='projectIssues')
)
UNION ALL
SELECT * FROM (
SELECT 'dependency' AS row_kind,json_object('dependencyId',e.dependency_id,'blocking',e.blocking_deliverable_id,'blocked',e.blocked_deliverable_id) AS payload FROM workspace_deliverable_dependencies e WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='deliverable' AND v.id=e.blocked_deliverable_id)
UNION ALL
SELECT 'issue' AS row_kind,json_object('issueId',i.issue_id,'deliverableId',i.deliverable_id,'title',i.title,'status',i.status) AS payload FROM workspace_issues i WHERE EXISTS(SELECT 1 FROM view_items v WHERE (v.kind='issue' AND v.id=i.issue_id) OR (v.kind='deliverable' AND v.id=i.deliverable_id))
UNION ALL
SELECT 'thread' AS row_kind,json_object('threadId',t.thread_id,'areaId',t.area_id,'title',t.title,'creator',t.creator,'state',t.state) AS payload FROM workspace_threads t WHERE t.thread_id IN (SELECT thread_id FROM selected_threads)
UNION ALL
SELECT 'member' AS row_kind,json_object('threadMemberId',m.thread_member_id,'threadId',m.thread_id,'actorIdentityId',m.actor_identity_id,'role',m.member_role,'notifications',CASE m.notifications WHEN 1 THEN 'on' WHEN 0 THEN 'off' ELSE NULL END,'state',json(CASE WHEN m.left_at IS NULL THEN '{"kind":"joined"}' ELSE json_object('kind','left','at',m.left_at) END)) AS payload FROM workspace_thread_members m WHERE (m.thread_id IN (SELECT thread_id FROM selected_threads) AND m.actor_identity_id=?4) OR EXISTS(SELECT 1 FROM view_items v WHERE v.kind='threadMember' AND v.id=m.thread_member_id)
)
UNION ALL
SELECT * FROM (
SELECT 'projectRole' AS row_kind,json_object('projectRoleId',r.project_role_id,'projectId',r.project_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) AS payload FROM workspace_project_roles r WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='projectRole' AND v.id=r.project_role_id)
UNION ALL
SELECT 'deliverableRole' AS row_kind,json_object('deliverableRoleId',r.deliverable_role_id,'deliverableId',r.deliverable_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) AS payload FROM workspace_deliverable_roles r WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='deliverableRole' AND v.id=r.deliverable_role_id)
UNION ALL
SELECT 'post' AS row_kind,json_object('postId',p.post_id,'threadId',p.thread_id,'author',p.author,'body',p.body,'addressees',p.addressees) AS payload FROM workspace_posts p WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='post' AND v.id=p.post_id)
UNION ALL
SELECT 'entry' AS row_kind,json_object('entryId',e.entry_id,'projectId',e.project_id,'author',e.author,'body',json(CASE e.redacted WHEN 1 THEN CASE WHEN e.body IS NULL THEN '{"kind":"redacted"}' ELSE NULL END WHEN 0 THEN json_object('kind','visible','body',e.body) ELSE NULL END),'tags',json((SELECT json_group_array(tag) FROM workspace_work_log_tags g WHERE g.entry_id=e.entry_id ORDER BY g.work_log_tag_id))) AS payload FROM workspace_work_log_entries e WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='workLogEntry' AND v.id=e.entry_id)
)
UNION ALL
SELECT * FROM (
SELECT 'backlog' AS row_kind,json_object('itemId',b.item_id,'areaId',b.area_id,'title',b.title,'description',b.description,'creator',b.creator,'status',b.status,'closingNote',b.closing_note) AS payload FROM workspace_backlog_items b WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='backlogItem' AND v.id=b.item_id)
UNION ALL
SELECT 'reference' AS row_kind,json_object('referenceId',r.reference_id,'source',json(json_object('kind',r.source_kind,'id',r.source_id)),'target',json(json_object('kind',r.target_kind,'id',r.target_id)),'details',r.details) AS payload FROM workspace_item_references r WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='reference' AND v.id=r.reference_id)
UNION ALL
SELECT 'link' AS row_kind,json_object('linkId',l.link_id,'source',json(json_object('kind',l.source_kind,'id',l.source_id)),'url',l.url,'title',l.title,'details',l.details) AS payload FROM workspace_links l WHERE EXISTS(SELECT 1 FROM view_items v WHERE v.kind='link' AND v.id=l.link_id)
UNION ALL
SELECT 'mark' AS row_kind,json_object('readMarkId',m.read_mark_id,'actorIdentityId',m.actor_identity_id,'scope',m.scope,'throughPosition',m.through_position) AS payload FROM workspace_read_marks m WHERE m.actor_identity_id=?4
)
UNION ALL
SELECT * FROM (
SELECT 'change' AS row_kind,json_object('changeId',c.change_id,'position',c.position,'item',json(json_object('kind',c.item_kind,'id',c.item_id)),'actor',c.actor,'changeKind',c.change_kind,'threadId',c.thread_id,'projectId',c.project_id,'subjectActor',c.subject_actor,'role',c.role) AS payload FROM workspace_changes c WHERE c.change_id IN (SELECT change_id FROM inbox_changes) OR EXISTS(SELECT 1 FROM view_items v WHERE v.kind=c.item_kind AND v.id=c.item_id AND c.change_kind IN ('created','posted'))
UNION ALL
SELECT 'listed' AS row_kind,json_object('item',json(json_object('kind',s.kind,'id',s.id)),'position',s.position) AS payload FROM selected_items s,request WHERE request.op='list'
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM workspace_changes
);
