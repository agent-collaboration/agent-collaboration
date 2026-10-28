SELECT
    p.participation_id AS "participation_id!: String",
    p.thread_id AS "thread_id!: String",
    p.actor_id AS "reader_actor!: String",
    a.canonical_identity AS "reader_identity?: String",
    p.joined_epoch AS "joined_epoch!: i64",
    p.joined_position AS "joined_position!: i64",
    p.role AS "role!: String",
    p.note AS "note?: String",
    p.last_seen_epoch AS "last_seen_epoch!: i64",
    p.last_seen_position AS "last_seen_position!: i64",
    p.closed_epoch AS "closed_epoch?: i64",
    p.closed_position AS "closed_position?: i64",
    p.closed_reason AS "closed_reason?: String",
    p.replaced_by AS "replaced_by?: String",
    b.canonical_identity AS "replacement_identity?: String"
FROM json_each(?) k CROSS JOIN thread_participation p ON p.thread_id=CAST(k.value AS TEXT)
LEFT JOIN actors a ON a.actor_id=p.actor_id
LEFT JOIN actors b ON b.actor_id=p.replaced_by
ORDER BY p.thread_id,p.joined_position,p.actor_id
