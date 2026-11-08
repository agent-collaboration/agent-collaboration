SELECT CAST(item_id AS TEXT) AS item_id, CAST(record_kind AS TEXT) AS record_kind, CAST(status_kind AS TEXT) AS status_kind FROM _fleet_sync_outbox_items ORDER BY item_id;
