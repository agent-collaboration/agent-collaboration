-- The latest person-within-organization fence central has received (r3 §6).
-- Its id is the sign-in service's retained subject UUIDv7 for the pair.
CREATE TABLE person_organization_authorization_floors (
    id TEXT NOT NULL PRIMARY KEY,
    organization_id TEXT NOT NULL,
    person_id TEXT NOT NULL,
    minimum_revision INTEGER NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (organization_id, person_id)
);
