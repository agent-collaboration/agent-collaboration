CREATE TABLE IF NOT EXISTS fleet_decision_receipts (
    receipt_id TEXT NOT NULL PRIMARY KEY,
    request_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    effect TEXT NOT NULL,
    UNIQUE(request_id, ordinal)
);
