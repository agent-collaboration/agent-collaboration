CREATE TABLE pending_poll_mail (
    item_id TEXT PRIMARY KEY NOT NULL,
    recipient_service_id TEXT NOT NULL,
    recipient_endpoint_id TEXT NOT NULL,
    recipient_session_id TEXT NOT NULL,
    membership_id TEXT NOT NULL,
    received_at TEXT NOT NULL
) STRICT;
CREATE INDEX pending_poll_mail_recipient ON pending_poll_mail (
    recipient_service_id, recipient_endpoint_id, recipient_session_id,
    received_at, item_id, membership_id
);
