CREATE TABLE owner_grants (
    grant_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project_state(project_id),
    kind TEXT NOT NULL DEFAULT 'ownerGrant',
    FOREIGN KEY(kind, grant_id) REFERENCES record_identities(kind, id)
);
-- Both engines retain this explicit foreign-key structure in their catalogs.
CREATE TABLE role_assignments_rebuilt (
    assignment_id TEXT PRIMARY KEY,
    project_subject TEXT,
    workstream_subject TEXT,
    role TEXT NOT NULL,
    active BOOLEAN NOT NULL CHECK(active IN (0, 1)),
    kind TEXT NOT NULL DEFAULT 'roleAssignment',
    owner_grant TEXT,
    FOREIGN KEY(kind, assignment_id) REFERENCES record_identities(kind, id),
    FOREIGN KEY(project_subject) REFERENCES project_state(project_id),
    FOREIGN KEY(workstream_subject) REFERENCES workstreams(workstream_id),
    FOREIGN KEY(owner_grant) REFERENCES owner_grants(grant_id)
);
INSERT INTO role_assignments_rebuilt(assignment_id, project_subject, workstream_subject, role, active, kind)
SELECT assignment_id, project_subject, workstream_subject, role, active, kind FROM role_assignments;
DROP TABLE role_assignments;
ALTER TABLE role_assignments_rebuilt RENAME TO role_assignments;
CREATE UNIQUE INDEX one_project_role_holder ON role_assignments(project_subject, role) WHERE active = 1;
CREATE UNIQUE INDEX one_workstream_role_holder ON role_assignments(workstream_subject, role) WHERE active = 1;
CREATE UNIQUE INDEX owner_grant_used_once ON role_assignments(owner_grant) WHERE owner_grant IS NOT NULL;
CREATE TABLE stand_down_acknowledgements (
    acknowledgement_id TEXT PRIMARY KEY,
    assignment_id TEXT NOT NULL REFERENCES role_assignments(assignment_id),
    through_epoch INTEGER NOT NULL,
    through_position INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'standDownAcknowledgement',
    FOREIGN KEY(kind, acknowledgement_id) REFERENCES record_identities(kind, id)
);
