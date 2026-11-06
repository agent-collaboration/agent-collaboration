CREATE TABLE fleet_decision_receipts (
    receipt_id TEXT NOT NULL PRIMARY KEY,
    request_id TEXT NOT NULL REFERENCES fleet_request_decisions(request_id),
    ordinal INTEGER NOT NULL,
    record_kind TEXT NOT NULL,
    record_id TEXT NOT NULL,
    fleet_position INTEGER NOT NULL,
    origin_installation TEXT NOT NULL,
    origin_machine TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(request_id, ordinal)
);
