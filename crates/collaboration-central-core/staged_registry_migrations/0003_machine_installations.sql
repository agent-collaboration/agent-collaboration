CREATE TABLE machine_installations (
    id TEXT NOT NULL PRIMARY KEY,
    former_service_id TEXT UNIQUE,
    label TEXT NOT NULL,
    enrollment_generation INTEGER NOT NULL,
    last_seen_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
DROP TABLE machine_enrollments;
