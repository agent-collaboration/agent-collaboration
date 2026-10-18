CREATE TABLE IF NOT EXISTS _session_state (singleton INTEGER PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS _central_continuations (call_id TEXT PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS pending_forwards (forward_id TEXT PRIMARY KEY, call_id TEXT NOT NULL UNIQUE, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS delivered_forwards (forward_id TEXT PRIMARY KEY, record TEXT NOT NULL);
