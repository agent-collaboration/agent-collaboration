CREATE TABLE record_identities (
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(kind, id)
);
CREATE TABLE project_state (
    project_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'project',
    FOREIGN KEY(kind, project_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE workstreams (
    workstream_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project_state(project_id),
    kind TEXT NOT NULL DEFAULT 'workstream',
    FOREIGN KEY(kind, workstream_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE tasks (
    task_id TEXT PRIMARY KEY,
    workstream_id TEXT NOT NULL REFERENCES workstreams(workstream_id),
    kind TEXT NOT NULL DEFAULT 'task',
    FOREIGN KEY(kind, task_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE task_links (
    from_task TEXT NOT NULL REFERENCES tasks(task_id),
    link_kind TEXT NOT NULL,
    to_task TEXT NOT NULL REFERENCES tasks(task_id),
    PRIMARY KEY(from_task, link_kind, to_task)
);
CREATE TABLE subtasks (
    subtask_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    place TEXT NOT NULL,
    checked BOOLEAN NOT NULL CHECK(checked IN (0, 1)),
    kind TEXT NOT NULL DEFAULT 'subtask',
    FOREIGN KEY(kind, subtask_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE work_trail_rows (
    row_id TEXT PRIMARY KEY,
    redacted BOOLEAN NOT NULL CHECK(redacted IN (0, 1)),
    kind TEXT NOT NULL DEFAULT 'workTrailRow',
    FOREIGN KEY(kind, row_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE work_trail_tags (
    row_id TEXT NOT NULL REFERENCES work_trail_rows(row_id),
    place INTEGER NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY(row_id, place)
);
CREATE TABLE role_assignments (
    assignment_id TEXT PRIMARY KEY,
    project_subject TEXT REFERENCES project_state(project_id),
    workstream_subject TEXT REFERENCES workstreams(workstream_id),
    role TEXT NOT NULL,
    active BOOLEAN NOT NULL CHECK(active IN (0, 1)),
    kind TEXT NOT NULL DEFAULT 'roleAssignment',
    FOREIGN KEY(kind, assignment_id) REFERENCES record_identities(kind, id)
);
CREATE UNIQUE INDEX one_project_role_holder ON role_assignments(project_subject, role) WHERE active = 1;
CREATE UNIQUE INDEX one_workstream_role_holder ON role_assignments(workstream_subject, role) WHERE active = 1;
CREATE TABLE threads (
    thread_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'thread',
    FOREIGN KEY(kind, thread_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE posts (
    post_id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    kind TEXT NOT NULL DEFAULT 'post',
    FOREIGN KEY(kind, post_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE read_marks (
    reader TEXT NOT NULL,
    scope TEXT NOT NULL,
    through_epoch INTEGER NOT NULL,
    through_position INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'readMark',
    record_id TEXT NOT NULL,
    PRIMARY KEY(reader, scope),
    FOREIGN KEY(kind, record_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE notify_intents (
    intent_id TEXT PRIMARY KEY,
    cause_kind TEXT NOT NULL,
    cause_id TEXT NOT NULL,
    recipient TEXT NOT NULL,
    assignment_revision INTEGER,
    state TEXT NOT NULL,
    supersedes TEXT REFERENCES notify_intents(intent_id),
    kind TEXT NOT NULL DEFAULT 'notifyIntent',
    FOREIGN KEY(kind, intent_id) REFERENCES record_identities(kind, id),
    FOREIGN KEY(cause_kind, cause_id) REFERENCES record_identities(kind, id)
);
CREATE UNIQUE INDEX one_intent_replacement ON notify_intents(supersedes, assignment_revision) WHERE supersedes IS NOT NULL;
CREATE TABLE request_ledger (
    request_id TEXT PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    observed_epoch INTEGER,
    accepted_in INTEGER,
    decision TEXT NOT NULL,
    resubmits TEXT,
    kind TEXT NOT NULL DEFAULT 'request',
    FOREIGN KEY(kind, request_id) REFERENCES record_identities(kind, id)
);
CREATE TABLE writer_epochs (
    epoch INTEGER PRIMARY KEY,
    holder TEXT NOT NULL,
    start_position INTEGER NOT NULL,
    ended TEXT
);
CREATE TABLE feed_checkpoint (
    singleton INTEGER PRIMARY KEY,
    epoch INTEGER NOT NULL,
    last_position INTEGER NOT NULL
);
INSERT INTO feed_checkpoint(singleton, epoch, last_position) VALUES (1, 1, 0);
CREATE TABLE project_feed (
    position INTEGER PRIMARY KEY,
    epoch INTEGER NOT NULL,
    record_kind TEXT NOT NULL,
    record_id TEXT NOT NULL,
    request_id TEXT,
    entry TEXT NOT NULL,
    FOREIGN KEY(record_kind, record_id) REFERENCES record_identities(kind, id)
);
