SELECT
    r.revision_id AS "record_id!: String",
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
    r.content_hash AS "content_hash!: String",
    r.document AS "document!: String",
    r.superseded_by AS "superseded_by?: String",
    r.revision AS "revision!: i64"
FROM json_each(?2) k CROSS JOIN plan_revisions r ON r.revision_id=CAST(k.value AS TEXT)
CROSS JOIN project_state p
LEFT JOIN actors a ON a.actor_id=r.actor_id
LEFT JOIN project_feed f ON f.epoch=r.epoch AND f.position=r.position
WHERE p.project_id=?1
ORDER BY r.revision_id
