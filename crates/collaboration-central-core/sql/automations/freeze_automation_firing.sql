UPDATE automation_firings SET batch_id=?,report_service_id=?,pending_entry_json=NULL WHERE firing_id=? AND state='pending' AND batch_id IS NULL AND report_service_id IS NULL;
