CREATE TABLE admin_imports (
    import_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    digest TEXT NOT NULL,
    header TEXT NOT NULL,
    phase TEXT NOT NULL,
    UNIQUE (project_id, digest)
);
