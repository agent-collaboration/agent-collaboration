SELECT import_id,service_id,automation_id,digest FROM automation_imports WHERE import_id=?1 OR automation_id=?2 ORDER BY import_id;
