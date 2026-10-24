CREATE TABLE router_pushes (
    push_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    origin_kind TEXT NOT NULL,
    origin_service_id TEXT,
    origin_endpoint_id TEXT,
    origin_session_id TEXT,
    origin_router_ref TEXT,
    target_service_id TEXT NOT NULL,
    target_endpoint_id TEXT NOT NULL,
    target_session_id TEXT NOT NULL,
    dm_delivery_mode TEXT,
    dm_generation_guard_json TEXT,
    reply_to_push_id TEXT,
    header_facts_json TEXT NOT NULL,
    body TEXT,
    ranges_json TEXT,
    delivery_state TEXT NOT NULL,
    last_outcome_json TEXT,
    cancelled_after_delivery_json TEXT,
    created_at TEXT NOT NULL,
    settled_at TEXT,
    read_at TEXT,
    retention TEXT NOT NULL DEFAULT 'creationAge'
) STRICT;
CREATE INDEX router_pushes_target_state ON router_pushes(
    target_service_id, target_endpoint_id, target_session_id, delivery_state, created_at
);
CREATE INDEX router_pushes_created ON router_pushes(created_at);
CREATE UNIQUE INDEX router_pushes_origin_ref ON router_pushes(origin_kind, origin_router_ref);
CREATE INDEX router_pushes_mail_retention ON router_pushes(retention, settled_at, push_id);

CREATE TABLE subscription_delivery_progress (
    progress_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    membership_id TEXT NOT NULL,
    activated_epoch INTEGER NOT NULL,
    activated_position INTEGER NOT NULL,
    payload TEXT NOT NULL
);
CREATE UNIQUE INDEX subscription_delivery_progress_identity ON subscription_delivery_progress(
    project_id, membership_id, activated_epoch, activated_position
);

CREATE TABLE board_intent_progress (
    progress_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    intent_id TEXT NOT NULL,
    state_epoch INTEGER NOT NULL,
    state_position INTEGER NOT NULL
);
CREATE UNIQUE INDEX board_intent_progress_identity ON board_intent_progress(
    project_id, intent_id, state_epoch, state_position
);

CREATE TABLE board_intent_scan_cursors (
    project_id TEXT PRIMARY KEY NOT NULL,
    epoch INTEGER NOT NULL,
    position INTEGER NOT NULL
);

CREATE TABLE local_cancel_deliveries (
    item_id TEXT PRIMARY KEY NOT NULL,
    target_item TEXT NOT NULL,
    attribution_json TEXT NOT NULL,
    outcome_json TEXT,
    recorded_at TEXT NOT NULL
);
