UPDATE machine_work_items SET state='received' WHERE work_item_id=? AND state IN ('pending','held');
