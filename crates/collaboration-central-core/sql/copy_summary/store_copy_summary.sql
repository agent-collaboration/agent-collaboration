INSERT INTO _copy_summary(database_id,record) VALUES(?,?)
ON CONFLICT(database_id) DO UPDATE SET record=excluded.record;
