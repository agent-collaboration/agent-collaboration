//! Expected application tables in a freshly migrated organization and workspace.

pub const APPLICATION_TABLE_NAMES: &str = "SELECT name FROM sqlite_schema WHERE type='table' AND substr(name,1,7)<>'sqlite_' AND name<>'_sqlx_migrations' ORDER BY name";

pub const ORGANIZATION_TABLES: &[&str] = &[
    "access_actor_identities",
    "admin_used_requests",
    "automation_schedules",
    "automation_wakes",
    "directory_workspaces",
    "engine_work_attempts",
    "login_machine_approvals",
    "login_machines",
    "mail_messages",
    "mail_notification_window_items",
    "mail_notification_windows",
    "mail_workspace_drains",
    "runtime_request_log",
    "signin_authorization_floors",
    "signin_change_records",
    "signin_organization_identity",
    "signin_person_authorization_floors",
];

pub const WORKSPACE_TABLES: &[&str] = &[
    "workspace_areas",
    "workspace_backlog_items",
    "workspace_changes",
    "workspace_deliverable_dependencies",
    "workspace_deliverable_roles",
    "workspace_deliverables",
    "workspace_initiatives",
    "workspace_issues",
    "workspace_item_references",
    "workspace_links",
    "workspace_posts",
    "workspace_project_roles",
    "workspace_projects",
    "workspace_read_marks",
    "workspace_request_log",
    "workspace_thread_members",
    "workspace_threads",
    "workspace_work_log_entries",
    "workspace_work_log_tags",
];
