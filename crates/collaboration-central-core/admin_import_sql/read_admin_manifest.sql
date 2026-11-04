SELECT CAST(row_id AS TEXT) AS row_id,
typeof(row_id) AS row_id_class,
CAST(import_id AS TEXT) AS import_id,
typeof(import_id) AS import_id_class,
CAST(ordinal AS TEXT) AS ordinal,
typeof(ordinal) AS ordinal_class,
CAST(table_name AS TEXT) AS table_name,
typeof(table_name) AS table_name_class,
CAST(row_count AS TEXT) AS row_count,
typeof(row_count) AS row_count_class,
CAST(table_digest AS TEXT) AS table_digest,
typeof(table_digest) AS table_digest_class,
CAST(created_at AS TEXT) AS created_at,
typeof(created_at) AS created_at_class,
CAST(updated_at AS TEXT) AS updated_at,
typeof(updated_at) AS updated_at_class
FROM fleet_admin_import_manifest_tables
WHERE import_id=? ORDER BY fleet_admin_import_manifest_tables.ordinal LIMIT CAST(?2 AS INTEGER);
