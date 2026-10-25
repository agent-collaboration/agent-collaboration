WITH selection AS (SELECT ? AS subject), selected AS (
 SELECT json_extract(subject,'$.kind') AS kind,json_extract(subject,'$.subject') AS id,
 CASE json_extract(subject,'$.kind') WHEN 'workstream' THEN json_extract(subject,'$.subject') WHEN 'task' THEN (SELECT workstream_id FROM tasks WHERE task_id=json_extract(subject,'$.subject')) END AS workstream FROM selection)
SELECT i.intent_id AS "id!: String" FROM notify_intents i CROSS JOIN selected s
LEFT JOIN role_assignments cause_role ON cause_role.assignment_id=i.cause_id
LEFT JOIN work_trail_rows cause_trail ON cause_trail.row_id=i.cause_id
LEFT JOIN posts cause_post ON cause_post.post_id=i.cause_id
LEFT JOIN threads thread ON thread.thread_id=cause_post.thread_id
LEFT JOIN work_trail_rows about_trail ON about_trail.row_id=CAST(json_extract(thread.about,'$.subject') AS TEXT)
WHERE i.intent_id>? AND (
 s.kind='project' OR i.cause_kind=s.kind AND i.cause_id=s.id
 OR (json_extract(CASE json_extract(i.recipient,'$.kind') WHEN 'resolved' THEN json_extract(i.recipient,'$.recipient.viaRole.subject') ELSE json_extract(i.recipient,'$.recipient.subject') END,'$.kind')='workstream' AND json_extract(CASE json_extract(i.recipient,'$.kind') WHEN 'resolved' THEN json_extract(i.recipient,'$.recipient.viaRole.subject') ELSE json_extract(i.recipient,'$.recipient.subject') END,'$.subject')=s.workstream)
 OR i.cause_kind='roleAssignment' AND cause_role.workstream_subject=s.workstream
 OR i.cause_kind='workTrailRow' AND (json_extract(cause_trail.subject,'$.kind')=s.kind AND json_extract(cause_trail.subject,'$.subject')=s.id OR s.kind='workstream' AND json_extract(cause_trail.subject,'$.kind')='task' AND EXISTS(SELECT 1 FROM tasks task WHERE task.task_id=json_extract(cause_trail.subject,'$.subject') AND task.workstream_id=s.id))
 OR i.cause_kind='post' AND ((json_extract(thread.about,'$.kind')=s.kind AND json_extract(thread.about,'$.subject')=s.id
 OR s.kind='workstream' AND json_extract(thread.about,'$.kind')='task' AND EXISTS(SELECT 1 FROM tasks task WHERE task.task_id=json_extract(thread.about,'$.subject') AND task.workstream_id=s.id))
 OR json_extract(thread.about,'$.kind')='workTrailRow' AND (json_extract(about_trail.subject,'$.kind')=s.kind AND json_extract(about_trail.subject,'$.subject')=s.id OR s.kind='workstream' AND json_extract(about_trail.subject,'$.kind')='task' AND EXISTS(SELECT 1 FROM tasks task WHERE task.task_id=json_extract(about_trail.subject,'$.subject') AND task.workstream_id=s.id))))
ORDER BY i.intent_id LIMIT 101
