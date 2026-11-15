//! Actual private machine approval/token contracts over native SQLite.
mod approval;
mod boundaries;
mod concurrency;
mod constraints;
mod fencing;
mod idle_fencing;
mod invariants;
mod liveness;
mod receipt;
mod revocation;
mod rollback;
mod stored_corruption;
mod support;
mod superseded_notice;
