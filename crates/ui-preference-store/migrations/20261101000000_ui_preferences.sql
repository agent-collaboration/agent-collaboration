CREATE TABLE ui_preferences (
    preference_id TEXT PRIMARY KEY NOT NULL,
    screen TEXT NOT NULL,
    setting TEXT NOT NULL,
    value TEXT NOT NULL,
    UNIQUE (screen, setting)
);
