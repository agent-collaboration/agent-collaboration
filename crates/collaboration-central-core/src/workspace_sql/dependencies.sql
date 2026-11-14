WITH request_context AS (SELECT ?1 AS workspace,?2 AS first_id,?3 AS second_id,?4 AS actor), selected_projects(project_id) AS (SELECT project_id FROM projects WHERE project_id IN (?2,?3) UNION SELECT project_id FROM deliverables WHERE deliverable_id IN (?2,?3) UNION SELECT d.project_id FROM deliverables d JOIN issues i ON i.deliverable_id=d.deliverable_id WHERE i.issue_id=?2), selected_threads(thread_id) AS (SELECT thread_id FROM threads WHERE thread_id IN (?2,?3) UNION SELECT thread_id FROM posts WHERE post_id IN (?2,?3))
SELECT 'project' AS row_kind,json_object('projectId',p.project_id,'areaId',p.area_id,'title',p.title,'status',p.status,'lead',(SELECT holder FROM project_roles r WHERE r.project_id=p.project_id AND r.role='lead' AND r.ended_at IS NULL)) AS payload FROM projects p WHERE p.project_id IN (SELECT project_id FROM selected_projects)
UNION ALL
SELECT 'deliverable' AS row_kind,json_object('deliverableId',d.deliverable_id,'projectId',d.project_id,'title',d.title) AS payload FROM deliverables d WHERE d.project_id IN (SELECT project_id FROM selected_projects) OR d.deliverable_id IN (?2,?3)
UNION ALL
SELECT 'dependency' AS row_kind,json_object('dependencyId',e.dependency_id,'blocking',e.blocking_deliverable_id,'blocked',e.blocked_deliverable_id) AS payload FROM deliverable_dependencies e WHERE e.blocking_deliverable_id IN (SELECT deliverable_id FROM deliverables WHERE project_id IN (SELECT project_id FROM selected_projects)) OR e.dependency_id=?2
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM changes;
