INSERT INTO fleet_feed_state (state_id, name, last_position, retained_through, created_at, updated_at) VALUES (?1, 'fleet', 0, 0, CAST(?2 AS INTEGER), CAST(?2 AS INTEGER));
