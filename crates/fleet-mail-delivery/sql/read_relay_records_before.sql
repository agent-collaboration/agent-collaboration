SELECT CAST(item_id AS TEXT) AS item_id FROM _fleet_sync_outbox_items WHERE typeof(item_id)='text' AND item_id<?1 ORDER BY item_id LIMIT CAST(?2 AS INTEGER);
