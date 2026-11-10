-- The update tool's ledger of one-time steps on this Router root. Temporary:
-- the follow-up that deletes the one-time steps drops it.
CREATE TABLE update_steps (
    id TEXT NOT NULL PRIMARY KEY,
    step INTEGER NOT NULL,
    source TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX update_steps_step_source ON update_steps(step, source);
