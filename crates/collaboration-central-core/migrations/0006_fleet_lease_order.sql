CREATE TABLE IF NOT EXISTS lease_incarnation_fence (singleton INTEGER PRIMARY KEY, incarnation TEXT NOT NULL);
INSERT OR IGNORE INTO lease_incarnation_fence(singleton,incarnation) VALUES(1,'0');
