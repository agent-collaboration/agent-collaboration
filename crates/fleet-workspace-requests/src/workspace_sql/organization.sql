WITH request_context AS (SELECT ?1 AS workspace,?2 AS first_id,?3 AS second_id,?4 AS actor), selected_projects(project_id) AS (SELECT project_id FROM projects WHERE project_id IN (?2,?3) UNION SELECT project_id FROM deliverables WHERE deliverable_id IN (?2,?3) UNION SELECT d.project_id FROM deliverables d JOIN issues i ON i.deliverable_id=d.deliverable_id WHERE i.issue_id=?2), selected_threads(thread_id) AS (SELECT thread_id FROM threads WHERE thread_id IN (?2,?3) UNION SELECT thread_id FROM posts WHERE post_id IN (?2,?3))
SELECT 'area' AS row_kind,json_object('areaId',a.area_id,'workspaceId',?1,'name',a.name,'state',a.state) AS payload FROM areas a WHERE a.area_id IN (?2,?3)
UNION ALL
SELECT 'project' AS row_kind,json_object('projectId',p.project_id,'areaId',p.area_id,'title',p.title,'status',p.status,'lead',(SELECT holder FROM project_roles r WHERE r.project_id=p.project_id AND r.role='lead' AND r.ended_at IS NULL)) AS payload FROM projects p WHERE p.project_id IN (SELECT project_id FROM selected_projects)
UNION ALL
SELECT 'initiative' AS row_kind,json_object('initiativeId',n.initiative_id,'name',n.name,'description',n.description,'status',n.status) AS payload FROM initiatives n WHERE n.initiative_id IN (?2,?3) OR n.initiative_id IN (SELECT source_id FROM item_references WHERE source_kind='initiative' AND target_kind='project' AND target_id IN (SELECT project_id FROM selected_projects))
UNION ALL
SELECT 'deliverable' AS row_kind,json_object('deliverableId',d.deliverable_id,'projectId',d.project_id,'title',d.title) AS payload FROM deliverables d WHERE d.project_id IN (SELECT project_id FROM selected_projects) OR d.deliverable_id IN (?2,?3)
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM changes;
