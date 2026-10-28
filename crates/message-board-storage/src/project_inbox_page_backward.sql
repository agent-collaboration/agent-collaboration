WITH inputs AS (SELECT ? AS reader_actor,? AS selected,? AS unread,? AS project,? AS sweep,? AS reader_state),
activity AS (
 SELECT a.*,CASE WHEN a.record_kind='thread' THEN a.record_id ELSE json_extract(a.content,'$.record.record.placement.threadId') END AS thread_id
 FROM project_feed a WHERE a.record_kind IN ('post','thread') AND a.position>? AND a.position<=?
), watched AS (
 SELECT a.*,i.reader_actor,i.selected,i.unread,i.project,i.sweep,
  CASE WHEN a.thread_id IS NULL THEN json_extract(i.reader_state,'$.main') ELSE json_extract(t.value,'$.mark') END AS mark,
  json_extract(t.value,'$.watch') AS watch
 FROM activity a CROSS JOIN inputs i
 LEFT JOIN json_each(json_extract(i.reader_state,'$.threads')) t ON CAST(t.key AS TEXT)=a.thread_id
)
SELECT a.position AS "position!: i64",a.epoch AS "epoch!: i64",a.record_kind AS "record_kind!: String",a.record_id AS "record_id!: String",
 a.router_time AS "router_time?: String",a.time_basis AS "time_basis!: String",
 CASE WHEN a.watch IS NULL THEN 0 ELSE 1 END AS "watch_active!: i64",
 json_extract(a.watch,'$.epoch') AS "watch_epoch?: i64",json_extract(a.watch,'$.position') AS "watch_position?: i64",
 json_extract(a.mark,'$.epoch') AS "mark_epoch?: i64",json_extract(a.mark,'$.position') AS "mark_position?: i64"
FROM watched a
WHERE (a.record_kind='post' OR (a.unread=1 AND json_extract(a.content,'$.record.record.state')!=(SELECT json_extract(previous.content,'$.record.record.state') FROM project_feed previous WHERE previous.record_kind='thread' AND previous.record_id=a.record_id AND previous.position<a.position ORDER BY previous.position DESC LIMIT 1)))
AND (a.thread_id IS NULL AND a.selected=1 OR a.thread_id IS NOT NULL AND a.watch IS NOT NULL)
AND (a.unread=0 OR (
 a.actor_id!=a.reader_actor
 AND (a.thread_id IS NULL AND a.mark IS NOT NULL AND json_extract(a.mark,'$.position')<a.position
  OR a.thread_id IS NOT NULL AND a.watch IS NOT NULL AND json_extract(a.watch,'$.position')<a.position)
 AND (a.mark IS NULL OR json_extract(a.mark,'$.position')<a.position)))
AND (a.sweep IS NULL OR (CASE a.time_basis WHEN 'legacy_unknown' THEN 0 ELSE 1 END,COALESCE(a.router_time,''),a.project,a.position)
 < (CASE json_extract(a.sweep,'$.time.kind') WHEN 'unknown' THEN 0 ELSE 1 END,COALESCE(json_extract(a.sweep,'$.time.value'),''),json_extract(a.sweep,'$.projectId'),json_extract(a.sweep,'$.position')))
ORDER BY a.position DESC LIMIT ?
