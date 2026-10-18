CREATE TABLE IF NOT EXISTS _copy_directory_outbox (update_key TEXT PRIMARY KEY,call_id TEXT NOT NULL UNIQUE,record TEXT NOT NULL);
ALTER TABLE _copy_state ADD COLUMN last_piece_at TEXT;
