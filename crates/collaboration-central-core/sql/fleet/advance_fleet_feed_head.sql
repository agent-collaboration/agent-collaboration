UPDATE fleet_feed_state SET last_position = CAST(?1 AS INTEGER), updated_at = CAST(?2 AS INTEGER) WHERE name = 'fleet';
