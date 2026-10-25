UPDATE fleet_feed_state SET last_position = CAST(? AS INTEGER) WHERE name = 'fleet';
