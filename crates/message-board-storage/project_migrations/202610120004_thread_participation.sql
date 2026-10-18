CREATE TABLE thread_participants (
    reader TEXT NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    joined_epoch INTEGER NOT NULL,
    joined_position INTEGER NOT NULL,
    role TEXT NOT NULL,
    active BOOLEAN NOT NULL CHECK(active IN (0, 1)),
    payload TEXT NOT NULL,
    PRIMARY KEY(reader, thread_id, joined_epoch, joined_position)
);
CREATE UNIQUE INDEX one_active_thread_participant ON thread_participants(reader, thread_id) WHERE active = 1;
CREATE UNIQUE INDEX one_unique_thread_seat ON thread_participants(thread_id, role) WHERE active = 1 AND role IN ('orchestrator', 'implementer');
CREATE TABLE thread_watches (
    reader TEXT NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    start_epoch INTEGER NOT NULL,
    start_position INTEGER NOT NULL,
    active BOOLEAN NOT NULL CHECK(active IN (0, 1)),
    PRIMARY KEY(reader, thread_id)
);
CREATE TABLE project_watches (
    reader TEXT PRIMARY KEY,
    start_epoch INTEGER NOT NULL,
    start_position INTEGER NOT NULL,
    active BOOLEAN NOT NULL CHECK(active IN (0, 1))
);
CREATE TABLE thread_subscriptions (
    reader TEXT NOT NULL,
    scope TEXT NOT NULL,
    membership_kind TEXT NOT NULL,
    membership_id TEXT NOT NULL,
    thread_id TEXT REFERENCES threads(thread_id),
    PRIMARY KEY(reader, scope),
    FOREIGN KEY(membership_kind, membership_id) REFERENCES record_identities(kind, id)
);
