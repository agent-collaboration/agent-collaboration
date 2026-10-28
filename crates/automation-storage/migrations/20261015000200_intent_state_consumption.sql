-- M2 is unreleased. Identity-only progress cannot identify a handled state version.
-- Discard this derived progress; source intents and push origins preserve recovery.
DROP TABLE board_intent_progress;
CREATE TABLE board_intent_progress (
    project_id TEXT NOT NULL,
    intent_id TEXT NOT NULL,
    state_epoch INTEGER NOT NULL,
    state_position INTEGER NOT NULL,
    PRIMARY KEY(project_id, intent_id, state_epoch, state_position)
);
