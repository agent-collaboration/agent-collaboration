CREATE TABLE areas (area_id TEXT PRIMARY KEY, name TEXT NOT NULL, state TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE projects (project_id TEXT PRIMARY KEY, area_id TEXT NOT NULL REFERENCES areas(area_id), title TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE initiatives (initiative_id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);

CREATE TABLE project_roles (project_role_id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(project_id), role TEXT NOT NULL, holder TEXT NOT NULL, granted_at TEXT NOT NULL, ended_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX project_roles_one_active ON project_roles(project_id, role) WHERE ended_at IS NULL;

CREATE TABLE deliverables (deliverable_id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(project_id), title TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE deliverable_roles (deliverable_role_id TEXT PRIMARY KEY, deliverable_id TEXT NOT NULL REFERENCES deliverables(deliverable_id), role TEXT NOT NULL, holder TEXT NOT NULL, granted_at TEXT NOT NULL, ended_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX deliverable_roles_one_active ON deliverable_roles(deliverable_id, role) WHERE ended_at IS NULL;
CREATE TABLE deliverable_dependencies (dependency_id TEXT PRIMARY KEY, blocking_deliverable_id TEXT NOT NULL REFERENCES deliverables(deliverable_id), blocked_deliverable_id TEXT NOT NULL REFERENCES deliverables(deliverable_id), created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX deliverable_dependencies_pair ON deliverable_dependencies(blocking_deliverable_id, blocked_deliverable_id);
CREATE INDEX deliverable_dependencies_by_blocked ON deliverable_dependencies(blocked_deliverable_id);
CREATE TABLE issues (issue_id TEXT PRIMARY KEY, deliverable_id TEXT NOT NULL REFERENCES deliverables(deliverable_id), title TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);

CREATE TABLE threads (thread_id TEXT PRIMARY KEY, area_id TEXT NOT NULL REFERENCES areas(area_id), title TEXT NOT NULL, creator TEXT NOT NULL, state TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE thread_members (thread_member_id TEXT PRIMARY KEY, thread_id TEXT NOT NULL REFERENCES threads(thread_id), actor_identity_id TEXT NOT NULL, member_role TEXT NOT NULL, notifications INTEGER NOT NULL CHECK (notifications IN (0, 1)), left_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX thread_members_current ON thread_members(thread_id, actor_identity_id) WHERE left_at IS NULL;
CREATE TABLE posts (post_id TEXT PRIMARY KEY, thread_id TEXT NOT NULL REFERENCES threads(thread_id), author TEXT NOT NULL, body TEXT NOT NULL, addressees TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX posts_by_thread ON posts(thread_id, post_id);

CREATE TABLE work_log_entries (entry_id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(project_id), author TEXT NOT NULL, body TEXT, redacted INTEGER NOT NULL CHECK (redacted IN (0, 1)), created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX work_log_by_project ON work_log_entries(project_id, entry_id);
CREATE TABLE work_log_tags (work_log_tag_id TEXT PRIMARY KEY, entry_id TEXT NOT NULL REFERENCES work_log_entries(entry_id), tag TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX work_log_tags_by_tag ON work_log_tags(tag, entry_id);

CREATE TABLE backlog_items (item_id TEXT PRIMARY KEY, area_id TEXT NOT NULL REFERENCES areas(area_id), title TEXT NOT NULL, description TEXT NOT NULL, creator TEXT NOT NULL, status TEXT NOT NULL, closing_note TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);

CREATE TABLE item_references (reference_id TEXT PRIMARY KEY, source_kind TEXT NOT NULL, source_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, details TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX item_references_by_source ON item_references(source_id, reference_id);
CREATE INDEX item_references_by_target ON item_references(target_kind, target_id);
CREATE TABLE links (link_id TEXT PRIMARY KEY, source_kind TEXT NOT NULL, source_id TEXT NOT NULL, url TEXT NOT NULL, title TEXT NOT NULL, details TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX links_by_source ON links(source_id, link_id);

-- E22: one row per change, in the same batch as the change; ordered by position, never by change_id. No record copy.
CREATE TABLE changes (change_id TEXT PRIMARY KEY, position INTEGER NOT NULL, change_kind TEXT NOT NULL, item_kind TEXT NOT NULL, item_id TEXT NOT NULL, thread_id TEXT, project_id TEXT, actor TEXT NOT NULL, subject_actor TEXT, role TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX changes_position ON changes(position);
CREATE INDEX changes_by_thread ON changes(thread_id, position);
CREATE INDEX changes_by_item ON changes(item_kind, item_id, position);
-- E23
CREATE TABLE read_marks (read_mark_id TEXT PRIMARY KEY, actor_identity_id TEXT NOT NULL, scope TEXT NOT NULL, through_position INTEGER NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE UNIQUE INDEX read_marks_scope ON read_marks(actor_identity_id, scope);

-- R31: a write's request id (UUIDv7, minted by the caller), its caller, a digest of its content and its first answer.
-- Written in the same batch as the write it records, so the write and its record land together. Pruned after 24 hours.
CREATE TABLE request_log (request_id TEXT PRIMARY KEY, caller TEXT NOT NULL, content_digest TEXT NOT NULL, answer TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX request_log_age ON request_log(created_at);
