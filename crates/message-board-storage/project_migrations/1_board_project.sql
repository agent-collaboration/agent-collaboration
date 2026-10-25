CREATE TABLE actors (
    actor_id TEXT NOT NULL PRIMARY KEY,
    canonical_identity TEXT NOT NULL UNIQUE
);
CREATE TABLE epoch_lines (
    line_id TEXT NOT NULL PRIMARY KEY,
    epoch INTEGER NOT NULL UNIQUE,
    inherited_through INTEGER NOT NULL
);
CREATE TABLE project_state (
    project_id TEXT NOT NULL PRIMARY KEY,
    description TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    board_id TEXT NOT NULL,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    revision INTEGER NOT NULL,
    closed_epoch INTEGER,
    closed_position INTEGER
);
CREATE TABLE workstreams (
    workstream_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project_state(project_id),
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    name TEXT NOT NULL,
    goal TEXT NOT NULL,
    state TEXT NOT NULL,
    revision INTEGER NOT NULL
);
CREATE TABLE tasks (
    task_id TEXT NOT NULL PRIMARY KEY,
    workstream_id TEXT NOT NULL REFERENCES workstreams(workstream_id),
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    status TEXT NOT NULL,
    revision INTEGER NOT NULL
);
CREATE INDEX tasks_by_workstream ON tasks(workstream_id, task_id);
CREATE TABLE task_links (
    link_id TEXT NOT NULL PRIMARY KEY,
    from_task TEXT NOT NULL REFERENCES tasks(task_id),
    link_kind TEXT NOT NULL,
    to_task TEXT NOT NULL REFERENCES tasks(task_id),
    UNIQUE(from_task, link_kind, to_task)
);
CREATE TABLE task_link_events (
    event_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    from_task TEXT NOT NULL REFERENCES tasks(task_id),
    link_kind TEXT NOT NULL,
    to_task TEXT NOT NULL REFERENCES tasks(task_id)
);
CREATE TABLE subtasks (
    subtask_id TEXT NOT NULL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    text TEXT NOT NULL,
    checked BOOLEAN NOT NULL CHECK(checked IN (0, 1)),
    removed BOOLEAN NOT NULL CHECK(removed IN (0, 1)),
    place TEXT NOT NULL
);
CREATE INDEX subtasks_by_task ON subtasks(task_id, place, subtask_id);
CREATE TABLE threads (
    thread_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    about TEXT NOT NULL,
    imported BOOLEAN NOT NULL CHECK(imported IN (0, 1)),
    state TEXT NOT NULL
);
CREATE TABLE thread_refilings (
    refiling_id TEXT NOT NULL PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    from_subject TEXT NOT NULL,
    to_subject TEXT NOT NULL
);
CREATE INDEX refilings_by_thread ON thread_refilings(thread_id, position, refiling_id);
CREATE TABLE posts (
    post_id TEXT NOT NULL PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    placement_kind TEXT NOT NULL,
    acting_for TEXT,
    text TEXT NOT NULL,
    posted_as_role TEXT
);
CREATE INDEX posts_by_thread ON posts(thread_id, position, post_id);
CREATE INDEX cooldown_posts ON posts(actor_id, placement_kind, router_time, post_id);
CREATE TABLE post_references (
    reference_id TEXT NOT NULL PRIMARY KEY,
    source_post TEXT NOT NULL REFERENCES posts(post_id),
    place INTEGER NOT NULL,
    target TEXT NOT NULL,
    UNIQUE(source_post, place)
);
CREATE TABLE thread_participation (
    participation_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    joined_epoch INTEGER NOT NULL,
    joined_position INTEGER NOT NULL,
    role TEXT NOT NULL,
    note TEXT,
    last_seen_epoch INTEGER NOT NULL,
    last_seen_position INTEGER NOT NULL,
    closed_epoch INTEGER,
    closed_position INTEGER,
    closed_reason TEXT,
    replaced_by TEXT REFERENCES actors(actor_id),
    UNIQUE(actor_id, thread_id, joined_epoch, joined_position)
);
CREATE INDEX participants_by_thread ON thread_participation(thread_id, actor_id, joined_epoch, joined_position);
CREATE TABLE owner_grants (
    grant_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    grant_kind TEXT NOT NULL,
    project_id TEXT NOT NULL REFERENCES project_state(project_id),
    new_holder TEXT NOT NULL REFERENCES actors(actor_id),
    reason TEXT NOT NULL,
    provenance_kind TEXT NOT NULL,
    relayed_by TEXT NOT NULL REFERENCES actors(actor_id)
);
CREATE TABLE role_assignments (
    assignment_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    project_subject TEXT REFERENCES project_state(project_id),
    workstream_subject TEXT REFERENCES workstreams(workstream_id),
    role TEXT NOT NULL,
    holder TEXT NOT NULL REFERENCES actors(actor_id),
    granted_by TEXT NOT NULL REFERENCES actors(actor_id),
    model TEXT,
    lineage TEXT,
    ended TEXT,
    owner_grant TEXT UNIQUE REFERENCES owner_grants(grant_id)
);
CREATE INDEX roles_by_project ON role_assignments(project_subject, role, assignment_id);
CREATE INDEX roles_by_workstream ON role_assignments(workstream_subject, role, assignment_id);
CREATE INDEX roles_by_holder ON role_assignments(holder, assignment_id);
CREATE TABLE stand_down_acknowledgements (
    acknowledgement_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    assignment_id TEXT NOT NULL REFERENCES role_assignments(assignment_id),
    through_epoch INTEGER NOT NULL,
    through_position INTEGER NOT NULL
);
CREATE INDEX acknowledgements_by_assignment ON stand_down_acknowledgements(assignment_id, through_epoch, through_position, acknowledgement_id);
CREATE TABLE work_trail_rows (
    row_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    subject TEXT NOT NULL,
    text TEXT NOT NULL,
    redacted BOOLEAN NOT NULL CHECK(redacted IN (0, 1))
);
CREATE TABLE work_trail_tags (
    tag_id TEXT NOT NULL PRIMARY KEY,
    row_id TEXT NOT NULL REFERENCES work_trail_rows(row_id),
    place INTEGER NOT NULL,
    tag TEXT NOT NULL,
    UNIQUE(row_id, place)
);
CREATE INDEX trail_by_tag ON work_trail_tags(tag, row_id);
CREATE TABLE work_trail_links (
    link_id TEXT NOT NULL PRIMARY KEY,
    row_id TEXT NOT NULL REFERENCES work_trail_rows(row_id),
    place INTEGER NOT NULL,
    target TEXT NOT NULL,
    UNIQUE(row_id, place)
);
CREATE TABLE facts (
    fact_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    fact TEXT NOT NULL,
    links TEXT NOT NULL,
    producer_time TEXT NOT NULL,
    anchor_epoch INTEGER,
    anchor_position INTEGER,
    late BOOLEAN NOT NULL CHECK(late IN (0, 1))
);
CREATE INDEX context_scope_facts ON facts(json_array(json_extract(fact, '$.fact.session.endpoint.serviceId'), json_extract(fact, '$.fact.session.endpoint.endpointId'), json_extract(fact, '$.fact.session.sessionId')), fact_id) WHERE json_extract(fact, '$.kind') = 'scopeAcknowledgement';
CREATE TABLE plan_revisions (
    revision_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    content_hash TEXT NOT NULL UNIQUE,
    document TEXT NOT NULL,
    superseded_by TEXT,
    revision INTEGER NOT NULL
);
CREATE TABLE read_marks (
    mark_id TEXT NOT NULL PRIMARY KEY,
    reader_actor TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    scope_kind TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    through_epoch INTEGER NOT NULL,
    through_position INTEGER NOT NULL,
    UNIQUE(reader_actor, scope_kind, scope_id)
);
CREATE TABLE thread_watches (
    watch_id TEXT NOT NULL PRIMARY KEY,
    reader_actor TEXT NOT NULL REFERENCES actors(actor_id),
    thread_id TEXT NOT NULL REFERENCES threads(thread_id),
    start_epoch INTEGER NOT NULL,
    start_position INTEGER NOT NULL,
    active BOOLEAN NOT NULL CHECK(active IN (0, 1)),
    UNIQUE(reader_actor, thread_id)
);
CREATE TABLE project_watches (
    watch_id TEXT NOT NULL PRIMARY KEY,
    reader_actor TEXT NOT NULL UNIQUE REFERENCES actors(actor_id),
    start_epoch INTEGER NOT NULL,
    start_position INTEGER NOT NULL,
    active BOOLEAN NOT NULL CHECK(active IN (0, 1))
);
CREATE TABLE subscription_memberships (
    membership_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    reader_actor TEXT NOT NULL REFERENCES actors(actor_id),
    scope_kind TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    policy TEXT NOT NULL,
    renewed_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    generation INTEGER NOT NULL,
    activated_epoch INTEGER NOT NULL,
    activated_position INTEGER NOT NULL,
    ended TEXT,
    UNIQUE(reader_actor, scope_kind, scope_id, generation)
);
CREATE INDEX subscriptions_by_scope ON subscription_memberships(scope_kind, scope_id, membership_id);
CREATE TABLE notify_intents (
    intent_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    request_id TEXT,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    cause_kind TEXT NOT NULL,
    cause_id TEXT NOT NULL,
    recipient_key TEXT NOT NULL,
    recipient TEXT NOT NULL,
    state TEXT NOT NULL,
    UNIQUE(cause_kind, cause_id, recipient_key)
);
CREATE TABLE mail_handoffs (
    item_id TEXT NOT NULL PRIMARY KEY,
    handoff_seq INTEGER NOT NULL UNIQUE,
    item TEXT NOT NULL
);
CREATE TABLE handoff_frontier (
    frontier_id TEXT NOT NULL PRIMARY KEY,
    settled_through INTEGER NOT NULL
);
CREATE TABLE mail_cancel_expansions (
    cancel_id TEXT NOT NULL PRIMARY KEY,
    expansion_seq INTEGER NOT NULL UNIQUE,
    group_handoff TEXT NOT NULL REFERENCES mail_handoffs(item_id),
    target_item TEXT NOT NULL REFERENCES mail_handoffs(item_id),
    envelope TEXT NOT NULL,
    UNIQUE(group_handoff, target_item)
);
CREATE TABLE request_decisions (
    request_id TEXT NOT NULL PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    canonical_hash TEXT NOT NULL,
    accepted_in INTEGER,
    decision TEXT NOT NULL
);
CREATE TABLE project_feed (
    entry_id TEXT NOT NULL PRIMARY KEY,
    epoch INTEGER NOT NULL REFERENCES epoch_lines(epoch),
    position INTEGER NOT NULL,
    record_kind TEXT NOT NULL,
    record_id TEXT NOT NULL,
    request_id TEXT,
    actor_id TEXT NOT NULL REFERENCES actors(actor_id),
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    router_time TEXT,
    time_basis TEXT NOT NULL,
    content TEXT NOT NULL,
    UNIQUE(epoch, position)
);
CREATE INDEX feed_by_record ON project_feed(record_kind, record_id, position);
CREATE INDEX feed_by_kind_position ON project_feed(record_kind, position);
CREATE INDEX feed_by_position ON project_feed(position);
CREATE INDEX feed_by_request_position ON project_feed(request_id, position);
