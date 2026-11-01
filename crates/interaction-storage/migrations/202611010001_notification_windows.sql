CREATE TABLE notification_windows (
    recipient_key TEXT PRIMARY KEY NOT NULL,
    last_delivered_at_ms INTEGER NOT NULL,
    flush_due_at_ms INTEGER
) STRICT;

ALTER TABLE router_pushes ADD COLUMN notification_listing_push_id TEXT REFERENCES router_pushes(push_id);
CREATE INDEX router_pushes_notification_listing ON router_pushes(notification_listing_push_id, push_id);
ALTER TABLE router_pushes ADD COLUMN notification_item_digest TEXT;
