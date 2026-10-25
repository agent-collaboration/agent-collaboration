INSERT INTO registry_notice_marks(database_id, mark) VALUES (?, ?) ON CONFLICT(database_id) DO UPDATE SET mark = excluded.mark;
