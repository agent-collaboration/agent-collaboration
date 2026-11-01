CREATE TABLE automations (
    automation_id TEXT NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL,
    revision TEXT NOT NULL,
    definition_json TEXT NOT NULL,
    anchor_at_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    imported_continuity_json TEXT,
    execution_mode TEXT,
    legacy_timing_json TEXT,
    state TEXT NOT NULL,
    creator_json TEXT NOT NULL,
    slot_session_key TEXT UNIQUE,
    fault_json TEXT,
    faulted_at_ms INTEGER
);

CREATE TABLE content_revisions (
    content_id TEXT NOT NULL PRIMARY KEY,
    owner_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    recorded_at_ms INTEGER NOT NULL,
    digest TEXT NOT NULL,
    text TEXT NOT NULL,
    UNIQUE(owner_id, revision_id)
);

CREATE TABLE instruction_documents (
    instruction_id TEXT NOT NULL PRIMARY KEY,
    current_content_id TEXT NOT NULL REFERENCES content_revisions(content_id),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE automation_batches (
    batch_id TEXT NOT NULL PRIMARY KEY,
    service_id TEXT NOT NULL,
    frozen_bytes TEXT NOT NULL,
    digest TEXT NOT NULL,
    notice_state TEXT NOT NULL,
    acked_at_ms INTEGER
);
CREATE INDEX automation_batches_by_machine ON automation_batches(service_id, batch_id);

CREATE TABLE automation_firings (
    firing_id TEXT NOT NULL PRIMARY KEY,
    automation_id TEXT NOT NULL REFERENCES automations(automation_id),
    due_ms INTEGER NOT NULL,
    revision TEXT NOT NULL,
    content_id TEXT NOT NULL REFERENCES content_revisions(content_id),
    state TEXT NOT NULL,
    batch_id TEXT REFERENCES automation_batches(batch_id) ON DELETE SET NULL,
    report_seq INTEGER NOT NULL,
    report_service_id TEXT,
    outcome_json TEXT,
    folded_through_ms INTEGER,
    pending_entry_json TEXT,
    UNIQUE(automation_id, due_ms)
);

CREATE TABLE automation_timing (
    automation_id TEXT NOT NULL PRIMARY KEY REFERENCES automations(automation_id),
    next_due_ms INTEGER,
    expires_at_ms INTEGER,
    admitted_through_ms INTEGER,
    imported_watermark_local_ms INTEGER,
    occupying_firing_id TEXT REFERENCES automation_firings(firing_id),
    waiting_firing_id TEXT REFERENCES automation_firings(firing_id),
    occupying_external_run TEXT
);
CREATE INDEX automation_timing_by_due ON automation_timing(next_due_ms);

CREATE TABLE automation_operations (
    operation_id TEXT NOT NULL PRIMARY KEY,
    digest TEXT NOT NULL,
    answer TEXT NOT NULL
);

CREATE TABLE automation_imports (
    import_id TEXT NOT NULL PRIMARY KEY,
    service_id TEXT NOT NULL,
    automation_id TEXT NOT NULL REFERENCES automations(automation_id),
    digest TEXT NOT NULL,
    UNIQUE(service_id, automation_id)
);

CREATE TABLE machine_work_capabilities (
    service_id TEXT NOT NULL PRIMARY KEY,
    protocols_json TEXT NOT NULL,
    observed_at_ms INTEGER NOT NULL
);
