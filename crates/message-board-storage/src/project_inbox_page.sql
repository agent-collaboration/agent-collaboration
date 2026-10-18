WITH inputs AS (SELECT ? AS reader, ? AS selected, ? AS unread, ? AS project, ? AS sweep)
SELECT a.position AS "position!: i64", a.epoch AS "epoch!: i64", a.record_kind AS "record_kind!: String", a.record_id AS "record_id!: String", json_extract(a.entry, '$.routerTime') AS "router_time!: String",
 COALESCE(w.active, p.active, 0) AS "watch_active!: i64",
 COALESCE(w.start_epoch, p.start_epoch) AS "watch_epoch: i64", COALESCE(w.start_position, p.start_position) AS "watch_position: i64",
 m.through_epoch AS "mark_epoch: i64", m.through_position AS "mark_position: i64"
FROM project_feed a INDEXED BY inbox_feed_by_position CROSS JOIN inputs i
LEFT JOIN thread_watches w ON w.reader = i.reader AND w.thread_id = (CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END)
LEFT JOIN project_watches p ON p.reader = i.reader AND (CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END) IS NOT NULL
LEFT JOIN read_marks m ON m.reader = i.reader AND m.scope = CASE WHEN (CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END) IS NULL
 THEN json_object('kind', 'project', 'projectId', i.project) ELSE json_object('kind', 'thread', 'threadId', (CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END)) END
WHERE a.record_kind IN ('post', 'thread') AND a.position > ? AND a.position <= ?
AND (a.record_kind = 'post' OR (i.unread = 1 AND json_extract(a.entry, '$.content.record.record.state') !=
 (SELECT json_extract(previous.entry, '$.content.record.record.state') FROM project_feed previous
  WHERE previous.record_kind = 'thread' AND previous.record_id = a.record_id AND previous.position < a.position ORDER BY previous.position DESC LIMIT 1)))
AND ((CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END) IS NULL AND i.selected = 1 OR (CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END) IS NOT NULL AND COALESCE(w.active, p.active, 0) != 0)
AND (i.unread = 0 OR (
 NOT (json_extract((CASE WHEN a.record_kind = 'thread' THEN json_extract(a.entry, '$.actor.actor.identity') ELSE json_extract(a.entry, '$.content.record.record.actor') END), '$.kind') IS json_extract(i.reader, '$.kind')
  AND json_extract((CASE WHEN a.record_kind = 'thread' THEN json_extract(a.entry, '$.actor.actor.identity') ELSE json_extract(a.entry, '$.content.record.record.actor') END), '$.humanId') IS json_extract(i.reader, '$.humanId')
  AND json_extract((CASE WHEN a.record_kind = 'thread' THEN json_extract(a.entry, '$.actor.actor.identity') ELSE json_extract(a.entry, '$.content.record.record.actor') END), '$.session.endpoint.serviceId') IS json_extract(i.reader, '$.session.endpoint.serviceId')
  AND json_extract((CASE WHEN a.record_kind = 'thread' THEN json_extract(a.entry, '$.actor.actor.identity') ELSE json_extract(a.entry, '$.content.record.record.actor') END), '$.session.endpoint.endpointId') IS json_extract(i.reader, '$.session.endpoint.endpointId')
  AND json_extract((CASE WHEN a.record_kind = 'thread' THEN json_extract(a.entry, '$.actor.actor.identity') ELSE json_extract(a.entry, '$.content.record.record.actor') END), '$.session.sessionId') IS json_extract(i.reader, '$.session.sessionId'))
 AND ((CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END) IS NULL AND m.through_epoch = a.epoch AND m.through_position < a.position
  OR (CASE WHEN a.record_kind = 'thread' THEN a.record_id ELSE json_extract(a.entry, '$.content.record.record.placement.threadId') END) IS NOT NULL AND COALESCE(w.start_epoch, p.start_epoch) = a.epoch AND COALESCE(w.start_position, p.start_position) < a.position)
 AND (m.through_epoch IS NULL OR m.through_epoch != a.epoch OR m.through_position < a.position)))
AND (i.sweep IS NULL OR (rtrim(json_extract(a.entry, '$.routerTime'), 'Z'), i.project, a.position)
 > (rtrim(json_extract(i.sweep, '$.routerTime'), 'Z'), json_extract(i.sweep, '$.projectId'), json_extract(i.sweep, '$.position')))
ORDER BY a.position ASC LIMIT ?
