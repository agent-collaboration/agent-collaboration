CREATE TABLE mail_submission_epochs (
    item_id TEXT PRIMARY KEY NOT NULL,
    project TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    recorded_at TEXT NOT NULL
) STRICT;
