//! **Superseded (2026-10-16).** This crate is a second, native application of central, with its
//! own routes, Access workflow, effect driver, alarms and transaction sequence. It runs on
//! Turso's engine, driven synchronously on blocking threads. One async core injected into every
//! runtime replaces it (requirements U69 to U75). The native runtime becomes
//! `collaboration-central-tokio`, which supplies platform capabilities only. See
//! `docs/specs/2026-10-16-central-one-core/what-went-wrong.md`.
#![forbid(unsafe_code)]
mod access_verification;
#[cfg(feature = "conformance")]
mod conformance_control;
mod http_server;
mod local_error;
mod native_effects;
mod object_actors;
mod platform_alarms;
mod session_runtime;
mod turso_storage;
pub use http_server::{LocalServer, LocalSettings, credential_hash};
pub use local_error::{LocalError, LocalResult};
pub use object_actors::ObjectActors;
pub use session_runtime::SessionRuntime;
