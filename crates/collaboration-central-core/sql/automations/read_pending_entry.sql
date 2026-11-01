SELECT firing_id,automation_id,CAST(due_ms AS TEXT) AS due_ms,revision,content_id,state,batch_id,pending_entry_json,report_service_id FROM automation_firings WHERE firing_id=?;
