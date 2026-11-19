WITH request_context AS (SELECT ?1 AS workspace,?2 AS first_id,?3 AS second_id,?4 AS actor), selected_projects(project_id) AS (SELECT project_id FROM projects WHERE project_id IN (?2,?3) UNION SELECT project_id FROM deliverables WHERE deliverable_id IN (?2,?3) UNION SELECT d.project_id FROM deliverables d JOIN issues i ON i.deliverable_id=d.deliverable_id WHERE i.issue_id=?2), selected_threads(thread_id) AS (SELECT thread_id FROM threads WHERE thread_id IN (?2,?3) UNION SELECT thread_id FROM posts WHERE post_id IN (?2,?3))
SELECT 'project' AS row_kind,json_object('projectId',p.project_id,'areaId',p.area_id,'title',p.title,'status',p.status,'lead',(SELECT holder FROM project_roles r WHERE r.project_id=p.project_id AND r.role='lead' AND r.ended_at IS NULL)) AS payload FROM projects p WHERE p.project_id IN (SELECT project_id FROM selected_projects)
UNION ALL
SELECT 'projectRole' AS row_kind,json_object('projectRoleId',r.project_role_id,'projectId',r.project_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) AS payload FROM project_roles r WHERE r.project_id IN (SELECT project_id FROM selected_projects)
UNION ALL
SELECT 'deliverable' AS row_kind,json_object('deliverableId',d.deliverable_id,'projectId',d.project_id,'title',d.title) AS payload FROM deliverables d WHERE d.project_id IN (SELECT project_id FROM selected_projects) OR d.deliverable_id IN (?2,?3)
UNION ALL
SELECT 'deliverableRole' AS row_kind,json_object('deliverableRoleId',r.deliverable_role_id,'deliverableId',r.deliverable_id,'role',r.role,'holder',r.holder,'grantedAt',r.granted_at,'state',json(CASE WHEN r.ended_at IS NULL THEN '{"kind":"active"}' ELSE json_object('kind','ended','at',r.ended_at) END)) AS payload FROM deliverable_roles r WHERE r.deliverable_id=?2
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM changes;
