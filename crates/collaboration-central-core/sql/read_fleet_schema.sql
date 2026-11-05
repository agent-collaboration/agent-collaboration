SELECT type AS object_kind, name AS object_name, tbl_name AS table_name, sql AS definition FROM sqlite_schema WHERE type IN ('table', 'index') ORDER BY type, name;
