CREATE TABLE pending_forwards (
    forward_frame TEXT PRIMARY KEY NOT NULL,
    record TEXT NOT NULL
);
CREATE TABLE delivered_forwards (
    forward_frame TEXT PRIMARY KEY NOT NULL,
    record TEXT NOT NULL
);
