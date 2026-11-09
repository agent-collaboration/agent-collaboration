CREATE TABLE IF NOT EXISTS central_work_attempts (
    id TEXT NOT NULL PRIMARY KEY,
    subject TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    send_id TEXT,
    connection_id TEXT,
    session_id TEXT,
    sent_version INTEGER,
    next_attempt_at INTEGER,
    attempts INTEGER NOT NULL,
    first_failed_at INTEGER,
    last_error_class TEXT,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(subject, kind),
    UNIQUE(send_id)
);
CREATE INDEX IF NOT EXISTS central_work_attempts_due ON central_work_attempts(state, next_attempt_at);
CREATE INDEX IF NOT EXISTS central_work_attempts_window ON central_work_attempts(connection_id, state);
