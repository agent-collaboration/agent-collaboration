SELECT
    r.task_id AS "record_id!: String",
    p.workspace_id AS "workspace_id!: String",
    p.board_id AS "board_id!: String",
    p.project_id AS "project_id!: String",
    r.actor_id AS "actor_id!: String",
    a.canonical_identity AS "actor_identity?: String",
    r.request_id AS "request_id?: String",
    r.epoch AS "epoch!: i64",
    r.position AS "position!: i64",
    r.router_time AS "router_time?: String",
    r.time_basis AS "time_basis!: String",
    f.entry_id AS "feed_entry_id?: String",
    f.record_id AS "feed_record_id?: String",
    f.record_kind AS "feed_record_kind?: String",
    f.actor_id AS "feed_actor_id?: String",
    f.request_id AS "feed_request_id?: String",
    f.epoch AS "feed_epoch?: i64",
    f.position AS "feed_position?: i64",
    f.router_time AS "feed_router_time?: String",
    f.time_basis AS "feed_time_basis?: String",
    f.origin_installation AS "origin_installation?: String",
    f.origin_machine AS "origin_machine?: String",
    r.workstream_id AS "workstream_id!: String",
    r.title AS "title!: String",
    r.body AS "body!: String",
    r.status AS "status!: String",
    r.revision AS "revision!: i64"
FROM json_each(?2) k CROSS JOIN tasks r ON r.task_id=CAST(k.value AS TEXT)
JOIN workstreams w ON w.workstream_id = r.workstream_id
JOIN project_state p ON p.project_id = w.project_id
LEFT JOIN actors a ON a.actor_id = r.actor_id
LEFT JOIN project_feed f ON f.epoch = r.epoch AND f.position = r.position
WHERE p.project_id = ?1
ORDER BY r.task_id
