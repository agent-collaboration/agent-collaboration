SELECT row_id,version,checksum,
CAST(created_at AS TEXT) AS created_at,typeof(created_at) AS created_class,
CAST(updated_at AS TEXT) AS updated_at,typeof(updated_at) AS updated_class,
CAST(applied_at AS TEXT) AS applied_at,typeof(applied_at) AS applied_class
FROM central_schema_migrations WHERE corpus=?1;
