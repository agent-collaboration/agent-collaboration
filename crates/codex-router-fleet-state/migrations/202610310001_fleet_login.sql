-- codex-router-fleet-state migration: 202610310001_fleet_login.sql.
-- One installation's confirmed public machine login. No credentials.
-- Central is reached through the saved sign-in address under /fleet/v1/session;
-- no learned per-organization endpoint or resource address is stored.
-- All URLs and identities are validated by Rust on write and stored-row read.
CREATE TABLE fleet_login (
    enrollment_id TEXT PRIMARY KEY NOT NULL,
    sign_in TEXT NOT NULL,
    organization_id TEXT NOT NULL,
    machine_environment TEXT NOT NULL,
    receiver_id TEXT NOT NULL,
    relay_peer_id TEXT NOT NULL
);

-- Relational singleton: every index entry has the same key. A later login
-- deletes/replaces this row in one transaction, with its new enrollment ID.
CREATE UNIQUE INDEX fleet_login_single_row ON fleet_login ((1));
