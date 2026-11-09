SELECT CAST(version AS INTEGER) AS version,description,CAST(success AS INTEGER) AS success,checksum,CAST(execution_time AS INTEGER) AS execution_time,
CAST(installed_on AS TEXT) AS installed_on,
typeof(version) AS version_class,typeof(success) AS success_class,typeof(checksum) AS checksum_class,
typeof(execution_time) AS execution_class,typeof(installed_on) AS installed_class
FROM _sqlx_migrations_central_session ORDER BY version;
