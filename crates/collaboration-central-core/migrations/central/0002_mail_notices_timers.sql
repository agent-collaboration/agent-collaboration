CREATE TABLE workspace_drains (workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id), drained_through_position INTEGER NOT NULL, drain_due_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX workspace_drains_due ON workspace_drains(drain_due_at) WHERE drain_due_at IS NOT NULL;
CREATE TABLE messages (message_id TEXT PRIMARY KEY, sender TEXT NOT NULL, target_machine_id TEXT NOT NULL REFERENCES machines(machine_id), target_session TEXT NOT NULL, message_kind TEXT NOT NULL, body TEXT NOT NULL, state TEXT NOT NULL, failure_reason TEXT, source_change_id TEXT, recipient_actor TEXT, source_workspace_id TEXT, source_thread_id TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX messages_queued_by_machine ON messages(target_machine_id, message_id) WHERE state = 'queued';
CREATE UNIQUE INDEX messages_one_per_source ON messages(source_change_id, recipient_actor) WHERE source_change_id IS NOT NULL;
CREATE INDEX messages_queued_by_thread ON messages(source_workspace_id, source_thread_id, recipient_actor) WHERE state = 'queued';
CREATE TABLE notification_windows (window_id TEXT PRIMARY KEY, recipient_actor TEXT NOT NULL, workspace_id TEXT NOT NULL, thread_id TEXT NOT NULL, first_activity_at TEXT NOT NULL, due_at TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX notification_windows_open ON notification_windows(recipient_actor, workspace_id, thread_id);
CREATE INDEX notification_windows_due ON notification_windows(due_at);
CREATE TABLE notification_window_items (window_item_id TEXT PRIMARY KEY, window_id TEXT NOT NULL REFERENCES notification_windows(window_id), change_id TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX notification_window_items_change ON notification_window_items(window_id, change_id);
CREATE TABLE wakes (wake_id TEXT PRIMARY KEY, target TEXT NOT NULL, due_at TEXT NOT NULL, text TEXT NOT NULL, state TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX wakes_due ON wakes(due_at) WHERE state = 'active';
CREATE TABLE automations (automation_id TEXT PRIMARY KEY, target TEXT NOT NULL, timing_rule TEXT NOT NULL, anchor_at TEXT NOT NULL, evaluated_through_at TEXT, text TEXT NOT NULL, next_due_at TEXT, state TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX automations_due ON automations(next_due_at) WHERE state = 'active' AND next_due_at IS NOT NULL;
-- At most one row: which organization this Fleet store belongs to. Only the
-- private provisioning link writes it; the applying transaction enforces the
-- single row (the primary key alone would admit a second organization).
CREATE TABLE organization_identity (
    id TEXT NOT NULL PRIMARY KEY,
    organization_statement TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
