CREATE INDEX IF NOT EXISTS fleet_feed_retention ON fleet_feed(as_of);
CREATE TABLE IF NOT EXISTS fleet_feed_retention_floor (singleton INTEGER PRIMARY KEY, through_position INTEGER NOT NULL);
INSERT OR IGNORE INTO fleet_feed_retention_floor(singleton,through_position) VALUES(1,0);
CREATE TABLE IF NOT EXISTS feed_readers (installation TEXT PRIMARY KEY, last_read_position INTEGER NOT NULL);
