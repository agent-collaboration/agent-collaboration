WITH input AS (SELECT ? AS reader, ? AS project_scope, ? AS epoch, ? AS selected), starts AS (
 SELECT m.through_position AS position FROM read_marks m, input i
 WHERE i.selected = 1 AND m.reader = i.reader AND m.scope = i.project_scope AND m.through_epoch = i.epoch
 UNION ALL
 SELECT CASE WHEN m.through_epoch = i.epoch AND m.through_position > w.start_position THEN m.through_position ELSE w.start_position END
 FROM input i JOIN thread_watches w ON w.reader = i.reader
 LEFT JOIN read_marks m ON m.reader = i.reader AND m.scope = json_object('kind', 'thread', 'threadId', w.thread_id)
 WHERE w.active != 0 AND w.start_epoch = i.epoch
 UNION ALL
 SELECT p.start_position FROM project_watches p, input i WHERE p.reader = i.reader AND p.active != 0 AND p.start_epoch = i.epoch
)
SELECT MIN(position) AS "position?: i64" FROM starts
