CREATE TABLE machine_forward_requests (
    id TEXT NOT NULL PRIMARY KEY,
    forward_frame_id TEXT NOT NULL,
    side TEXT NOT NULL,
    request_frame_id TEXT NOT NULL,
    origin_machine_id TEXT NOT NULL,
    origin_session_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    expected_holder_machine_id TEXT,
    project_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    deadline_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(forward_frame_id, side)
);
CREATE INDEX machine_forward_origins ON machine_forward_requests(origin_machine_id, side);
CREATE INDEX machine_forward_deadlines ON machine_forward_requests(deadline_at);
