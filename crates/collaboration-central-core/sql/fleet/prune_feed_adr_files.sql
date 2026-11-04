DELETE FROM fleet_feed_adr_repository_files WHERE entry_id IN (SELECT entry_id FROM fleet_feed WHERE as_of<=?);
