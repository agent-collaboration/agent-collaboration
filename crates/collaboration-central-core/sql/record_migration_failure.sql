INSERT INTO central_schema_migration_failures(row_id,corpus,version,reason,created_at,updated_at,failed_at)
VALUES(?1,?2,?3,?4,CAST(?5 AS INTEGER),CAST(?5 AS INTEGER),CAST(?5 AS INTEGER))
ON CONFLICT(corpus) DO UPDATE SET version=excluded.version,reason=excluded.reason,
updated_at=excluded.updated_at,failed_at=excluded.failed_at;
