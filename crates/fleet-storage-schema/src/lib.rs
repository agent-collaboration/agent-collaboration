//! The shared migration registries. Each host applies them at store open.
#![forbid(unsafe_code)]

mod workspace_migrations;

#[cfg(feature = "test-support")]
pub use workspace_migrations::failing_upgrade;
pub use workspace_migrations::{central_migrator, workspace_migrator};
