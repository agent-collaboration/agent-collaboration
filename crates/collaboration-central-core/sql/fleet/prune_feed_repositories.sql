DELETE FROM fleet_feed_repository_refs WHERE entry_id IN (SELECT entry_id FROM fleet_feed WHERE as_of<=?);
