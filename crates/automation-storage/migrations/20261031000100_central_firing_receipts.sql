-- Central owns timing; local rows are projections for the existing drivers.
ALTER TABLE schedule_definitions ADD COLUMN authority TEXT NOT NULL DEFAULT 'local';
ALTER TABLE wakeup_definitions ADD COLUMN authority TEXT NOT NULL DEFAULT 'local';
CREATE TABLE firing_receipts (
    firing_id TEXT NOT NULL PRIMARY KEY,
    batch_id TEXT NOT NULL,
    local_work_ref TEXT NOT NULL,
    state TEXT NOT NULL,
    report_seq INTEGER NOT NULL
) STRICT;
CREATE TABLE central_report_outbox (
    report_id TEXT NOT NULL PRIMARY KEY,
    subject_kind TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    report_seq INTEGER NOT NULL,
    report_json TEXT NOT NULL,
    queued_at_ms INTEGER NOT NULL,
    UNIQUE(subject_kind, subject_id, report_seq)
) STRICT;
