SELECT row_id,version,description,success,checksum,execution_time,
typeof(version) AS version_class,typeof(success) AS success_class,typeof(execution_time) AS execution_class,
CAST(created_at AS TEXT) AS created_at,typeof(created_at) AS created_class,
CAST(updated_at AS TEXT) AS updated_at,typeof(updated_at) AS updated_class,
CAST(installed_on AS TEXT) AS installed_on,typeof(installed_on) AS installed_class
FROM central_schema_migrations WHERE corpus=?1;
