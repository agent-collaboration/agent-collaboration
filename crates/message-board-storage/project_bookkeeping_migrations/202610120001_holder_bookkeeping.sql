CREATE TABLE _replication_state (
    singleton INTEGER PRIMARY KEY,
    epoch INTEGER NOT NULL,
    start_epoch INTEGER NOT NULL,
    start_commit_id INTEGER NOT NULL,
    acknowledged_epoch INTEGER NOT NULL,
    acknowledged_commit_id INTEGER NOT NULL,
    acknowledged_schema_version INTEGER NOT NULL
);
CREATE TABLE _replication_feed_marks (
    epoch INTEGER NOT NULL,
    commit_id INTEGER NOT NULL,
    feed_position INTEGER NOT NULL,
    PRIMARY KEY(epoch, commit_id)
);
