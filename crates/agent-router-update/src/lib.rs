//! One-time Router root updates and legacy board translation, separate from the running Router.
mod import_digest;
mod import_error;
mod import_keys;
mod legacy_reader;
mod legacy_rows;
mod legacy_schema;
mod message_activity_map;
mod message_copy;
pub use message_activity_map::MessageActivityMap;
mod message_copy_sql;
mod message_legacy;
mod message_origin_map;
mod message_rows;
pub use message_copy::{
    MESSAGE_COPY_DIGEST_BASIS, MessageCopyCounts, MessageUpgrade, MessageUpgradeError,
    MessageUpgradeReport,
};
mod ledger_step;
mod merge_step;
mod project_transform;
mod rename_step;
mod root_layout;
mod router_update;
pub use root_layout::RouterUpdateError;
pub use router_update::{
    PendingUpdateStep, PlannedChange, RouterUpdatePlan, plan_router_update, run_router_update,
};
mod update_barrier;
mod upgrade_prepare;
mod upgrade_report;
mod upgrade_upload;

pub use import_digest::IMPORT_DIGEST_BASIS;
pub use import_error::UpgradeError;
pub use import_keys::DerivedImportKeys;
pub use upgrade_prepare::{PreparedProject, PreparedUpgrade, prepare_sources};
pub use upgrade_report::{
    BoardImportCounts, CollisionLocation, NameCollision, ProjectImportReport, UpgradeReport,
    WorkspaceImportCounts,
};
pub use upgrade_upload::{
    ProjectUploadReport, UpgradePublisher, UpgradeUploadError, UpgradeUploadReport,
};
