//! Executes Fleet statement batches on stock SQLx SQLite.
#![forbid(unsafe_code)]

mod sqlite_object_store;

pub use sqlite_object_store::{AlarmChange, SqliteObjectStore, StoreOpenFailure};
