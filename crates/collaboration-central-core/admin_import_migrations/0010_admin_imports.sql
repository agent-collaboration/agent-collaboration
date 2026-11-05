CREATE TABLE fleet_admin_imports (
    import_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    digest TEXT NOT NULL,
    provenance_installation TEXT NOT NULL,
    provenance_machine TEXT NOT NULL,
    declared_owner TEXT NOT NULL,
    manifest_epoch TEXT NOT NULL,
    manifest_sequence TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    schema_fingerprint TEXT NOT NULL,
    pages INTEGER NOT NULL,
    manifest_table_count INTEGER NOT NULL,
    summary_kind TEXT NOT NULL,
    summary_time_basis TEXT NOT NULL,
    summary_record_at TEXT,
    phase_kind TEXT NOT NULL,
    published_epoch TEXT,
    published_sequence TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id,digest)
);
CREATE TABLE fleet_admin_import_manifest_tables (
    row_id TEXT NOT NULL PRIMARY KEY,
    import_id TEXT NOT NULL REFERENCES fleet_admin_imports(import_id),
    ordinal INTEGER NOT NULL,
    table_name TEXT NOT NULL,
    row_count TEXT NOT NULL,
    table_digest TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(import_id,ordinal)
);
