WITH request_context AS (SELECT ?1 AS workspace,?2 AS first_id,?3 AS second_id,?4 AS actor), selected_projects(project_id) AS (SELECT project_id FROM projects WHERE project_id IN (?2,?3) UNION SELECT project_id FROM deliverables WHERE deliverable_id IN (?2,?3) UNION SELECT d.project_id FROM deliverables d JOIN issues i ON i.deliverable_id=d.deliverable_id WHERE i.issue_id=?2), selected_threads(thread_id) AS (SELECT thread_id FROM threads WHERE thread_id IN (?2,?3) UNION SELECT thread_id FROM posts WHERE post_id IN (?2,?3))
SELECT * FROM (
SELECT 'thread' AS row_kind,json_object('threadId',t.thread_id,'areaId',t.area_id,'title',t.title,'creator',t.creator,'state',t.state) AS payload FROM threads t WHERE t.thread_id IN (SELECT thread_id FROM selected_threads)
UNION ALL
SELECT 'member' AS row_kind,json_object('threadMemberId',m.thread_member_id,'threadId',m.thread_id,'actorIdentityId',m.actor_identity_id,'role',m.member_role,'notifications',CASE m.notifications WHEN 1 THEN 'on' WHEN 0 THEN 'off' ELSE NULL END,'state',json(CASE WHEN m.left_at IS NULL THEN '{"kind":"joined"}' ELSE json_object('kind','left','at',m.left_at) END)) AS payload FROM thread_members m WHERE m.thread_id IN (SELECT thread_id FROM selected_threads)
UNION ALL
SELECT 'area' AS row_kind,json_object('areaId',a.area_id,'workspaceId',?1,'name',a.name,'state',a.state) AS payload FROM areas a WHERE a.area_id IN (?2,?3)
UNION ALL
SELECT 'projectRole' AS row_kind,json_object('projectRoleId',r.project_role_id,'projectId',r.project_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) AS payload FROM project_roles r WHERE r.project_id IN (SELECT json_extract(value,'$.projectId') FROM json_each(?3) WHERE json_extract(value,'$.kind')='projectRole')
)
UNION ALL
SELECT * FROM (
SELECT 'deliverableRole' AS row_kind,json_object('deliverableRoleId',r.deliverable_role_id,'deliverableId',r.deliverable_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) AS payload FROM deliverable_roles r WHERE r.deliverable_id IN (SELECT json_extract(value,'$.deliverableId') FROM json_each(?3) WHERE json_extract(value,'$.kind')='deliverableRole')
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM changes
);
