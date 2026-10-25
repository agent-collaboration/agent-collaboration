CREATE TABLE capture_feed_marks (
    epoch INTEGER NOT NULL,
    captured_position INTEGER NOT NULL,
    first_feed_position INTEGER,
    last_feed_position INTEGER,
    max_handoff_seq INTEGER,
    max_expansion_seq INTEGER,
    settled_through INTEGER,
    snapshot_identity TEXT,
    PRIMARY KEY(epoch, captured_position)
);
