CREATE TABLE installation_peer_directory (
    peer_id TEXT PRIMARY KEY NOT NULL,
    service_id TEXT NOT NULL UNIQUE
) STRICT;
