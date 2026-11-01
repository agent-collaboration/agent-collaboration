UPDATE automation_firings SET state='held' WHERE batch_id=? AND state='pending';
