CREATE TABLE thread_refilings (
    refiling_id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    kind TEXT NOT NULL DEFAULT 'threadRefiled',
    FOREIGN KEY(kind, refiling_id) REFERENCES record_identities(kind, id)
);
CREATE INDEX refilings_by_thread ON thread_refilings(thread_id, refiling_id);
CREATE TABLE thread_subjects (
    thread_id TEXT PRIMARY KEY REFERENCES threads(thread_id),
    subject_kind TEXT NOT NULL,
    subject_id TEXT
);
