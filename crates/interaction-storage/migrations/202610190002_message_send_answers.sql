CREATE TABLE message_send_answers (
    item_id TEXT PRIMARY KEY NOT NULL,
    target_identity TEXT NOT NULL,
    result TEXT,
    recorded_at TEXT NOT NULL
) STRICT;
