SELECT CAST(item_id AS TEXT) AS item_id FROM _fleet_sync_outbox_items WHERE typeof(item_id)='text' ORDER BY item_id LIMIT 1;
