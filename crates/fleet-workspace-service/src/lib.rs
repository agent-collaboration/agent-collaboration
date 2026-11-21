//! Procedures that execute against the workspace's own SQLite.
#![forbid(unsafe_code)]

mod workspace_repository;
mod workspace_service;

pub use workspace_service::WorkspaceService;
