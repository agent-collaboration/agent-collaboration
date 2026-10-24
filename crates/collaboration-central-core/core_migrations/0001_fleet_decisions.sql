CREATE TABLE fleet_request_decisions (
    request_id TEXT NOT NULL PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    payload TEXT NOT NULL,
    router_time TEXT NOT NULL,
    time_basis TEXT NOT NULL
);
