//! The library applies these holder-local migrations outside capture.
use sqlx::migrate::Migrator;
pub static PROJECT_LOCAL_MIGRATOR: Migrator = sqlx::migrate!("./project_bookkeeping_migrations");
pub(crate) const LOCAL_TABLES: &[&str] = &["capture_feed_marks"];
