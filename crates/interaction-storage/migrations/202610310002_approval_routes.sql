CREATE TABLE approval_routes (
    thread_id TEXT PRIMARY KEY NOT NULL,
    route_json TEXT NOT NULL,
    last_used_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX approval_routes_last_used ON approval_routes(last_used_at_ms, thread_id);
