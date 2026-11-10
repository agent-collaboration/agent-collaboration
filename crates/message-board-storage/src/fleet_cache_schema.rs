//! The Fleet database's migrations, shared by the Host's store and the update ledger.
pub(crate) static CACHE_MIGRATOR: sqlx::migrate::Migrator =
    sqlx::migrate!("./fleet_cache_migrations");
