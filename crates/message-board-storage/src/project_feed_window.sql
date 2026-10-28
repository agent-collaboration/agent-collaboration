WITH cursor_row AS (
    SELECT COUNT(*) AS row_count, MIN(epoch) AS assigning_epoch
    FROM project_feed WHERE position = ?
)
SELECT
    c.row_count AS "cursor_row_count!: i64",
    c.assigning_epoch AS "cursor_epoch?: i64",
    f.entry_id AS "entry_id?: String",
    f.epoch AS "epoch?: i64",
    f.position AS "position?: i64",
    f.record_kind AS "record_kind?: String",
    f.record_id AS "record_id?: String",
    f.request_id AS "request_id?: String",
    f.actor_id AS "actor_id?: String",
    a.canonical_identity AS "actor_identity?: String",
    f.origin_installation AS "origin_installation?: String",
    f.origin_machine AS "origin_machine?: String",
    f.router_time AS "router_time?: String",
    f.time_basis AS "time_basis?: String",
    f.content AS "content?: String",
    CASE WHEN f.record_kind = 'workTrailRow' THEN t.redacted END AS "redacted?: i64"
FROM cursor_row c
LEFT JOIN project_feed f ON f.position > ? AND f.position <= ?
LEFT JOIN actors a ON a.actor_id = f.actor_id
LEFT JOIN work_trail_rows t ON t.row_id = f.record_id
ORDER BY f.position
