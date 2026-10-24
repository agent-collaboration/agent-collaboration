INSERT INTO delivered_forwards(forward_frame, record) VALUES(?, ?) ON CONFLICT(forward_frame) DO UPDATE SET record=excluded.record;
