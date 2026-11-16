-- One failure-only record per engine subject. Rust validates states and values.
CREATE TABLE central_work_attempts (
    attempt_id          TEXT PRIMARY KEY,
    machine_kind        TEXT NOT NULL,
    subject_key         TEXT NOT NULL,
    state               TEXT NOT NULL,
    failure_count       INTEGER NOT NULL,
    next_try_at         TEXT,
    last_failure        TEXT NOT NULL,
    last_failure_detail TEXT,
    first_failed_at     TEXT NOT NULL,
    last_failed_at      TEXT NOT NULL,
    retired_at          TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE UNIQUE INDEX central_work_attempts_subject
    ON central_work_attempts(machine_kind, subject_key);
CREATE INDEX central_work_attempts_state
    ON central_work_attempts(machine_kind, state, next_try_at);
