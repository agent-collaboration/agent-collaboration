INSERT INTO central_schema_migrations(row_id,corpus,version,checksum,created_at,updated_at,applied_at)
VALUES(?1,?2,?3,?4,CAST(?5 AS INTEGER),CAST(?5 AS INTEGER),CAST(?5 AS INTEGER));
