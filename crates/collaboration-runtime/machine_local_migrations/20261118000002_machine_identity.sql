-- One Router install owns the same UUIDv7 identity here and at central.
CREATE TABLE machine_identity (
 machine_id TEXT PRIMARY KEY NOT NULL, -- The Router install's sole UUIDv7 identity.
 created_at INTEGER NOT NULL, -- First creation in UTC Unix ms from the injected clock.
 updated_at INTEGER NOT NULL -- Latest row change in UTC Unix ms from the same clock.
);
CREATE UNIQUE INDEX machine_identity_singleton ON machine_identity ((1));
