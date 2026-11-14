WITH request_context AS (SELECT ?1 AS workspace,?2 AS first_id,?3 AS second_id,?4 AS actor), relation_sources(id) AS (SELECT source_id FROM item_references WHERE reference_id=?2 UNION SELECT source_id FROM links WHERE link_id=?2), selected_projects(project_id) AS (SELECT project_id FROM projects WHERE project_id IN (?2,?3,(SELECT id FROM relation_sources)) UNION SELECT project_id FROM deliverables WHERE deliverable_id IN (?2,?3,(SELECT id FROM relation_sources)) UNION SELECT d.project_id FROM deliverables d JOIN issues i ON i.deliverable_id=d.deliverable_id WHERE i.issue_id=?2), selected_threads(thread_id) AS (SELECT thread_id FROM threads WHERE thread_id IN (?2,?3,(SELECT id FROM relation_sources)) UNION SELECT thread_id FROM posts WHERE post_id IN (?2,?3,(SELECT id FROM relation_sources)))
SELECT * FROM (
SELECT 'area' AS row_kind,json_object('areaId',a.area_id,'workspaceId',?1,'name',a.name,'state',a.state) AS payload FROM areas a WHERE a.area_id IN (?2,?3,(SELECT id FROM relation_sources))
UNION ALL
SELECT 'project' AS row_kind,json_object('projectId',p.project_id,'areaId',p.area_id,'title',p.title,'status',p.status,'lead',(SELECT holder FROM project_roles r WHERE r.project_id=p.project_id AND r.role='lead' AND r.ended_at IS NULL)) AS payload FROM projects p WHERE p.project_id IN (SELECT project_id FROM selected_projects)
UNION ALL
SELECT 'initiative' AS row_kind,json_object('initiativeId',n.initiative_id,'name',n.name,'description',n.description,'status',n.status) AS payload FROM initiatives n WHERE n.initiative_id IN (?2,?3,(SELECT id FROM relation_sources)) OR n.initiative_id IN (SELECT source_id FROM item_references WHERE source_kind='initiative' AND target_kind='project' AND target_id IN (SELECT project_id FROM selected_projects))
UNION ALL
SELECT 'deliverable' AS row_kind,json_object('deliverableId',d.deliverable_id,'projectId',d.project_id,'title',d.title) AS payload FROM deliverables d WHERE d.project_id IN (SELECT project_id FROM selected_projects) OR d.deliverable_id IN (?2,?3,(SELECT id FROM relation_sources))
)
UNION ALL
SELECT * FROM (
SELECT 'issue' AS row_kind,json_object('issueId',i.issue_id,'deliverableId',i.deliverable_id,'title',i.title,'status',i.status) AS payload FROM issues i WHERE i.deliverable_id IN (SELECT deliverable_id FROM deliverables WHERE project_id IN (SELECT project_id FROM selected_projects)) OR i.issue_id IN (?2,?3,(SELECT id FROM relation_sources))
UNION ALL
SELECT 'thread' AS row_kind,json_object('threadId',t.thread_id,'areaId',t.area_id,'title',t.title,'creator',t.creator,'state',t.state) AS payload FROM threads t WHERE t.thread_id IN (SELECT thread_id FROM selected_threads)
UNION ALL
SELECT 'post' AS row_kind,json_object('postId',p.post_id,'threadId',p.thread_id,'author',p.author,'body',p.body,'addressees',p.addressees) AS payload FROM posts p WHERE p.post_id IN (?2,?3,(SELECT id FROM relation_sources))
UNION ALL
SELECT 'entry' AS row_kind,json_object('entryId',e.entry_id,'projectId',e.project_id,'author',e.author,'body',json(CASE e.redacted WHEN 1 THEN CASE WHEN e.body IS NULL THEN '{"kind":"redacted"}' ELSE NULL END WHEN 0 THEN json_object('kind','visible','body',e.body) ELSE NULL END),'tags',json((SELECT json_group_array(tag) FROM work_log_tags g WHERE g.entry_id=e.entry_id ORDER BY g.work_log_tag_id))) AS payload FROM work_log_entries e WHERE e.entry_id IN (?2,?3,(SELECT id FROM relation_sources))
)
UNION ALL
SELECT * FROM (
SELECT 'backlog' AS row_kind,json_object('itemId',b.item_id,'areaId',b.area_id,'title',b.title,'description',b.description,'creator',b.creator) AS payload FROM backlog_items b WHERE b.item_id IN (?2,?3,(SELECT id FROM relation_sources))
UNION ALL
SELECT 'reference' AS row_kind,json_object('referenceId',r.reference_id,'source',json(json_object('kind',r.source_kind,'id',r.source_id)),'target',json(json_object('kind',r.target_kind,'id',r.target_id)),'details',r.details) AS payload FROM item_references r WHERE r.reference_id=?2
UNION ALL
SELECT 'link' AS row_kind,json_object('linkId',l.link_id,'source',json(json_object('kind',l.source_kind,'id',l.source_id)),'url',l.url,'title',l.title,'details',l.details) AS payload FROM links l WHERE l.link_id=?2
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM changes
);
