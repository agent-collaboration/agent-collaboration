CREATE TABLE _copy_state (
    singleton INTEGER PRIMARY KEY,
    project TEXT NOT NULL,
    holder TEXT,
    fence_epoch TEXT NOT NULL,
    start_epoch TEXT NOT NULL,
    start_commit TEXT NOT NULL,
    acked_epoch TEXT NOT NULL,
    acked_commit TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    schema_fingerprint TEXT NOT NULL,
    archived INTEGER NOT NULL CHECK (archived IN (0, 1))
);
CREATE TABLE _copy_receipts (
    epoch TEXT NOT NULL,
    from_epoch TEXT NOT NULL,
    from_commit TEXT NOT NULL,
    through_epoch TEXT NOT NULL,
    through_commit TEXT NOT NULL,
    digest TEXT NOT NULL,
    PRIMARY KEY (epoch, from_epoch, from_commit, through_epoch, through_commit)
);
CREATE TABLE _copy_part_receipts (
    epoch TEXT NOT NULL,
    from_epoch TEXT NOT NULL,
    from_commit TEXT NOT NULL,
    through_epoch TEXT NOT NULL,
    through_commit TEXT NOT NULL,
    part_index INTEGER NOT NULL,
    digest TEXT NOT NULL,
    PRIMARY KEY (epoch, from_epoch, from_commit, through_epoch, through_commit, part_index)
);
CREATE TABLE _copy_staged_parts (
    transaction_epoch TEXT NOT NULL,
    transaction_commit TEXT NOT NULL,
    part_index INTEGER NOT NULL,
    staged_at TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY (transaction_epoch, transaction_commit, part_index)
);
CREATE TABLE _copy_transfers (
    transfer_id TEXT PRIMARY KEY,
    old_epoch TEXT NOT NULL,
    new_epoch TEXT NOT NULL,
    start_epoch TEXT NOT NULL,
    start_commit TEXT NOT NULL,
    old_holder TEXT
);
CREATE TABLE _copy_schema_steps (
    migration_version TEXT NOT NULL,
    statement_index INTEGER NOT NULL,
    statement TEXT NOT NULL,
    PRIMARY KEY (migration_version, statement_index)
);
CREATE TABLE _copy_summary (
    singleton INTEGER PRIMARY KEY,
    status TEXT NOT NULL,
    latest_record_at TEXT NOT NULL,
    position_epoch TEXT NOT NULL,
    position_commit TEXT NOT NULL,
    acked_at TEXT NOT NULL
);

CREATE TABLE _copy_import_pages (page_index INTEGER PRIMARY KEY, record TEXT NOT NULL);
CREATE TABLE _copy_import_metadata (singleton INTEGER PRIMARY KEY, record TEXT NOT NULL);
