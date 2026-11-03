SELECT row_id,corpus,version,reason,
CAST(created_at AS TEXT) AS created_at,typeof(created_at) AS created_class,
CAST(updated_at AS TEXT) AS updated_at,typeof(updated_at) AS updated_class,
CAST(failed_at AS TEXT) AS failed_at,typeof(failed_at) AS failed_class
FROM central_schema_migration_failures ORDER BY corpus;
