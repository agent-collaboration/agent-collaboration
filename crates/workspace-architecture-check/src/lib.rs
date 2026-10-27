//! Best-effort ratchet for cargo-modules' resolved item dependencies.
//!
//! This observes default-feature, native lib targets (bins for bin-only packages).
//! Fully qualified body paths and `super::` paths can be missed by cargo-modules.
//! Inherent methods are attributed to their type's item path, which need not be
//! the source module containing the implementation.

mod cycle_baseline;
mod dependency_graph;
mod error;
mod workspace_scan;

pub use cycle_baseline::{CycleBaseline, PackageGraphs, RatchetViolation};
pub use dependency_graph::{ItemEdge, ModuleEdge, ModuleGraph, parse_use_edges};
pub use error::ArchitectureError;
pub use workspace_scan::{WorkspaceScan, scan_workspace};
