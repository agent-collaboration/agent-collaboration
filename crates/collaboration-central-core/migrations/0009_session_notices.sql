CREATE TABLE IF NOT EXISTS _session_notice_head (singleton INTEGER PRIMARY KEY, last_position INTEGER NOT NULL, pruned_through INTEGER NOT NULL);
INSERT OR IGNORE INTO _session_notice_head(singleton,last_position,pruned_through) VALUES(1,0,0);
CREATE TABLE IF NOT EXISTS _session_notices (position INTEGER PRIMARY KEY, kind TEXT NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS _session_notice_expiry ON _session_notices(expires_at);
