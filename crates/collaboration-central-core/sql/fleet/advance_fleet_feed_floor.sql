UPDATE fleet_feed_state SET retained_through = CAST(?1 AS INTEGER), updated_at = CAST(?2 AS INTEGER) WHERE name = 'fleet';
