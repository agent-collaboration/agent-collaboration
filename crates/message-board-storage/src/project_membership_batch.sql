SELECT
    r.membership_id AS "record_id!: String",
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
    r.reader_actor AS "reader_actor!: String",
    b.canonical_identity AS "reader_identity?: String",
    r.scope_kind AS "scope_kind!: String",
    r.scope_id AS "scope_id!: String",
    r.policy AS "policy!: String",
    r.renewed_at AS "renewed_at!: String",
    r.expires_at AS "expires_at!: String",
    r.generation AS "generation!: i64",
    r.activated_epoch AS "activated_epoch!: i64",
    r.activated_position AS "activated_position!: i64",
    r.ended AS "ended?: String"
FROM json_each(?2) k CROSS JOIN subscription_memberships r ON r.membership_id=CAST(k.value AS TEXT)
CROSS JOIN project_state p
LEFT JOIN actors a ON a.actor_id=r.actor_id
LEFT JOIN project_feed f ON f.epoch=r.epoch AND f.position=r.position
LEFT JOIN actors b ON b.actor_id=r.reader_actor
WHERE p.project_id=?1
ORDER BY r.membership_id
