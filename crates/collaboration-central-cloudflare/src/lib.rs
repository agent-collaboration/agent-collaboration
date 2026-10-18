//! **Superseded structure (2026-10-16).** This crate's application duplicates
//! `collaboration-central-local`'s, and it moves into the one async core:
//! - the hand-written routes in `worker_edge.rs`;
//! - the Access workflow;
//! - `effect_driver.rs`;
//! - the alarm policy;
//! - the synchronous `SqlTx` over Durable Object SQL.
//!
//! This crate keeps only Cloudflare capabilities, and serves the core's axum Router through
//! workers-rs (`router.call`). Requirements U69 to U75; see `docs/specs/2026-10-16-central-one-core/program-design.md`.
#![forbid(unsafe_code)]
mod access_verification;
mod cloudflare_storage;
#[cfg(feature = "conformance")]
mod conformance_control;
#[cfg(feature = "conformance-faults")]
mod conformance_faults;
#[cfg(feature = "conformance")]
mod conformance_object_control;
#[cfg(feature = "conformance")]
mod conformance_storage;
mod effect_driver;
mod face_alarms;
mod object_auth;
mod object_context;
mod object_rpc;
mod object_shell;
mod storage_measurement;
mod worker_edge;
pub use object_shell::{ConnectionObject, FleetObject, LeaseRegistryObject, ProjectCopyObject};
use worker::{Context, Env, Request, Response, Result, event};
#[event(fetch)]
pub async fn main(request: Request, env: Env, _context: Context) -> Result<Response> {
    worker_edge::fetch(request, env).await
}
