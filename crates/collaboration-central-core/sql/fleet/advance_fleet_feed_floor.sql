UPDATE fleet_feed_state SET retained_through = CAST(? AS INTEGER) WHERE name = 'fleet';
