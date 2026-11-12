//! The SQLx migrations each object kind runs when it opens.
//!
//! Both are empty in this slice: core's `migrations/central/` and
//! `migrations/workspace/` arrive with their tables, after the owner's review of the
//! workspace storage design. Opening still runs the stock migrator, so the history
//! table exists and the first real migration lands through the same path.

use sqlx_core::migrate::Migrator;
use std::sync::{Arc, LazyLock};

static CENTRAL_OBJECT: LazyLock<Arc<Migrator>> =
    LazyLock::new(|| Arc::new(Migrator::with_migrations(Vec::new())));
static WORKSPACE_STORE: LazyLock<Arc<Migrator>> =
    LazyLock::new(|| Arc::new(Migrator::with_migrations(Vec::new())));

/// Central's own database, on the Fleet object.
pub(crate) fn central_object() -> Arc<Migrator> {
    Arc::clone(&CENTRAL_OBJECT)
}

/// Every workspace store.
pub(crate) fn workspace_store() -> Arc<Migrator> {
    Arc::clone(&WORKSPACE_STORE)
}
