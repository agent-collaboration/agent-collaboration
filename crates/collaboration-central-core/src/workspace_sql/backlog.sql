WITH request_context AS (SELECT ?1 AS workspace,?2 AS first_id,?3 AS second_id,?4 AS actor), selected_projects(project_id) AS (SELECT project_id FROM workspace_projects WHERE project_id IN (?2,?3) UNION SELECT project_id FROM workspace_deliverables WHERE deliverable_id IN (?2,?3) UNION SELECT d.project_id FROM workspace_deliverables d JOIN workspace_issues i ON i.deliverable_id=d.deliverable_id WHERE i.issue_id=?2), selected_threads(thread_id) AS (SELECT thread_id FROM workspace_threads WHERE thread_id IN (?2,?3) UNION SELECT thread_id FROM workspace_posts WHERE post_id IN (?2,?3))
SELECT 'area' AS row_kind,json_object('areaId',a.area_id,'workspaceId',?1,'name',a.name,'description',a.description,'state',a.state) AS payload FROM workspace_areas a WHERE a.area_id IN (?2,?3)
UNION ALL
SELECT 'backlog' AS row_kind,json_object('itemId',b.item_id,'areaId',b.area_id,'title',b.title,'description',b.description,'creator',b.creator,'status',b.status,'closingNote',b.closing_note) AS payload FROM workspace_backlog_items b WHERE b.item_id IN (?2,?3)
UNION ALL
SELECT 'highWater' AS row_kind,CAST(COALESCE(MAX(position),0) AS TEXT) AS payload FROM workspace_changes;
