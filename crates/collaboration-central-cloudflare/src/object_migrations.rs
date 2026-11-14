//! The SQLx migrations each object kind runs when it opens.
//!
//! Legacy Fleet has no statement schema. WorkspaceStore uses the approved business
//! schema, and the widget apparatus selects its own explicit test-only fixture.

use sqlx_core::migrate::Migrator;
use std::sync::{Arc, LazyLock};

static CENTRAL_OBJECT: LazyLock<Arc<Migrator>> =
    LazyLock::new(|| Arc::new(Migrator::with_migrations(Vec::new())));
static WORKSPACE_STORE: LazyLock<Arc<Migrator>> = LazyLock::new(|| {
    Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator())
});
/// The explicit widget apparatus selects this fixture, never the business runtime.
#[cfg(feature = "test-support")]
static WIDGET_STORE: LazyLock<Arc<Migrator>> =
    LazyLock::new(|| Arc::new(collaboration_central_core::storage_conformance::widgets_migrator()));

/// Central's own database, on the Fleet object.
pub(crate) fn central_object() -> Arc<Migrator> {
    Arc::clone(&CENTRAL_OBJECT)
}

/// Every workspace store.
pub(crate) fn workspace_store() -> Arc<Migrator> {
    Arc::clone(&WORKSPACE_STORE)
}
#[cfg(feature = "test-support")]
pub(crate) fn widgets_store() -> Arc<Migrator> {
    Arc::clone(&WIDGET_STORE)
}
