INSERT INTO central_schema_migration_failures(row_id,corpus,version,reason,at) VALUES(?1,?2,?3,?4,?5)
ON CONFLICT(corpus) DO UPDATE SET version=excluded.version,reason=excluded.reason,at=excluded.at;
