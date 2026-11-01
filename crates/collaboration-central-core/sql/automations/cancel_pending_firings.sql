UPDATE automation_firings SET state='cancelled',outcome_json=NULL,pending_entry_json=NULL WHERE automation_id=? AND state='pending' AND batch_id IS NULL;
