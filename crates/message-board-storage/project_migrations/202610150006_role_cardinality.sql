DROP INDEX one_project_role_holder;
DROP INDEX one_workstream_role_holder;
CREATE UNIQUE INDEX one_project_role_holder ON role_assignments(project_subject, role) WHERE active = 1 AND role = 'lead';
CREATE UNIQUE INDEX one_workstream_role_holder ON role_assignments(workstream_subject, role) WHERE active = 1 AND role IN ('implementer', 'verifier');
CREATE INDEX active_project_roles ON role_assignments(project_subject, role, assignment_id) WHERE active = 1;
CREATE INDEX active_workstream_roles ON role_assignments(workstream_subject, role, assignment_id) WHERE active = 1;
