//! Stock SQLx execution of Fleet statement batches.
#![forbid(unsafe_code)]

mod sqlite_object_store;

pub use sqlite_object_store::{AlarmChange, SqliteObjectStore, StoreOpenFailure};
