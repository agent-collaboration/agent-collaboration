WITH selected AS (SELECT json_extract(subject,'$.kind') AS kind,json_extract(subject,'$.subject') AS id FROM (SELECT ? AS subject))
SELECT r.row_id AS "id!: String" FROM work_trail_rows r CROSS JOIN selected s
WHERE r.redacted=0 AND (s.kind='project'
 OR json_extract(r.subject,'$.kind')=s.kind AND json_extract(r.subject,'$.subject')=s.id
 OR s.kind='workstream' AND json_extract(r.subject,'$.kind')='task' AND EXISTS(SELECT 1 FROM tasks task WHERE task.task_id=json_extract(r.subject,'$.subject') AND task.workstream_id=s.id))
ORDER BY r.position DESC,r.row_id DESC LIMIT 21
