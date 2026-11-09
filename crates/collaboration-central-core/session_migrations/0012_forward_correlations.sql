CREATE TABLE machine_project_requests (
    id TEXT NOT NULL PRIMARY KEY,
    request_frame_id TEXT NOT NULL,
    requester_machine_id TEXT NOT NULL,
    requester_session_id TEXT NOT NULL,
    requester_connection_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    holder_machine_id TEXT,
    phase TEXT NOT NULL,
    deadline_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX machine_project_requesters ON machine_project_requests(requester_machine_id, phase);
CREATE INDEX machine_project_request_deadlines ON machine_project_requests(deadline_at);
