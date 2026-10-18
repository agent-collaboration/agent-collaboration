CREATE TABLE IF NOT EXISTS notice_outbox (installation TEXT NOT NULL, kind TEXT NOT NULL, owed_at TEXT NOT NULL, next_attempt_at TEXT NOT NULL, call_id TEXT UNIQUE, PRIMARY KEY(installation,kind));
CREATE INDEX IF NOT EXISTS notice_outbox_deadline ON notice_outbox(next_attempt_at);
