CREATE TABLE mail_read_contexts (
    item_id TEXT PRIMARY KEY NOT NULL,
    context_json TEXT NOT NULL,
    conversation_key TEXT,
    recorded_at TEXT NOT NULL
) STRICT;
CREATE INDEX mail_read_context_conversation ON mail_read_contexts(conversation_key, recorded_at DESC, item_id DESC);
