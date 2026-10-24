-- C5's local progress is separate from replicated intent state and its request ledger.
CREATE TABLE board_intent_progress (
    project_id TEXT NOT NULL,
    intent_id TEXT NOT NULL,
    PRIMARY KEY (project_id, intent_id)
);
