CREATE TABLE machine_work_items (
    work_item_id TEXT NOT NULL PRIMARY KEY,
    operation_id TEXT NOT NULL UNIQUE,
    service_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    request_json TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    state TEXT NOT NULL,
    result_json TEXT
);
