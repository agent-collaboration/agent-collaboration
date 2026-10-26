use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::cycle_baseline::PackageGraphs;
use crate::{ArchitectureError, ModuleGraph};

#[derive(Deserialize)]
struct WorkspaceMetadata {
    packages: Vec<MetadataPackage>,
    workspace_members: BTreeSet<String>,
}

#[derive(Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    targets: Vec<MetadataTarget>,
}

#[derive(Deserialize)]
struct MetadataTarget {
    name: String,
    kind: Vec<String>,
}

#[derive(Debug)]
enum TargetSelector {
    Library,
    Binary(String),
}

struct PackageTarget {
    package_name: String,
    crate_name: String,
    selector: TargetSelector,
}

pub struct WorkspaceScan {
    pub packages: PackageGraphs,
    pub elapsed: Duration,
    pub target_count: usize,
}

pub fn scan_workspace(root: &Path, jobs: NonZeroUsize) -> Result<WorkspaceScan, ArchitectureError> {
    let start = Instant::now();
    let version = run_command(root, &["modules", "--version"])?;
    if version.trim() != "cargo-modules 0.28.0" {
        return Err(ArchitectureError::ToolVersion(version.trim().to_owned()));
    }
    let metadata = run_command(root, &["metadata", "--format-version", "1", "--no-deps"])?;
    let metadata: WorkspaceMetadata =
        serde_json::from_str(&metadata).map_err(|source| ArchitectureError::Json {
            context: "decode cargo metadata",
            source,
        })?;
    let targets = workspace_targets(metadata);
    let mut packages = PackageGraphs::new();
    // Scoped batches bound the number of cargo-modules children without shared
    // mutable queues. Every worker is joined, including when one scan fails.
    for batch in targets.chunks(jobs.get()) {
        let results = std::thread::scope(|scope| {
            #[expect(
                clippy::needless_collect,
                reason = "spawn the entire batch before joining; lazy iteration serializes scans"
            )]
            let workers: Vec<_> = batch
                .iter()
                .map(|target| scope.spawn(move || scan_target(root, target)))
                .collect();
            workers
                .into_iter()
                .map(|worker| {
                    worker
                        .join()
                        .map_err(|_| ArchitectureError::WorkerPanicked)
                        .and_then(|result| result)
                })
                .collect::<Vec<_>>()
        });
        for (target, result) in batch.iter().zip(results) {
            packages
                .entry(target.package_name.clone())
                .or_default()
                .push(result?);
        }
    }
    Ok(WorkspaceScan {
        packages,
        elapsed: start.elapsed(),
        target_count: targets.len(),
    })
}

fn workspace_targets(metadata: WorkspaceMetadata) -> Vec<PackageTarget> {
    let mut targets = Vec::new();
    for package in metadata.packages {
        if !metadata.workspace_members.contains(&package.id) {
            continue;
        }
        let has_library = package.targets.iter().any(is_library);
        for target in package.targets {
            let selector = if is_library(&target) {
                TargetSelector::Library
            } else if !has_library && target.kind.iter().any(|kind| kind == "bin") {
                TargetSelector::Binary(target.name.clone())
            } else {
                continue;
            };
            targets.push(PackageTarget {
                package_name: package.name.clone(),
                crate_name: target.name.replace('-', "_"),
                selector,
            });
        }
    }
    targets.sort_by(|left, right| {
        (&left.package_name, &left.crate_name).cmp(&(&right.package_name, &right.crate_name))
    });
    targets
}

fn is_library(target: &MetadataTarget) -> bool {
    target.kind.iter().any(|kind| {
        matches!(
            kind.as_str(),
            "lib" | "rlib" | "cdylib" | "staticlib" | "proc-macro" | "dylib"
        )
    })
}

fn scan_target(root: &Path, target: &PackageTarget) -> Result<ModuleGraph, ArchitectureError> {
    let start = Instant::now();
    let mut arguments = vec![
        "modules",
        "dependencies",
        "-p",
        &target.package_name,
        "--no-externs",
        "--no-sysroot",
        "--no-owns",
    ];
    match &target.selector {
        TargetSelector::Library => arguments.push("--lib"),
        TargetSelector::Binary(name) => arguments.extend(["--bin", name.as_str()]),
    }
    let dot = run_command(root, &arguments)?;
    let graph = ModuleGraph::from_dot(&dot, &target.crate_name)?;
    eprintln!(
        "{} ({}): {:.3}s, {} SCC(s)",
        target.package_name,
        target.crate_name,
        start.elapsed().as_secs_f64(),
        graph.cycle_components().len()
    );
    Ok(graph)
}

fn run_command(root: &Path, arguments: &[&str]) -> Result<String, ArchitectureError> {
    let command = format!("cargo {}", arguments.join(" "));
    let output = Command::new("cargo")
        .current_dir(root)
        .args(arguments)
        .output()
        .map_err(|source| ArchitectureError::CommandIo {
            command: command.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(ArchitectureError::CommandFailed {
            command,
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    String::from_utf8(output.stdout)
        .map_err(|source| ArchitectureError::OutputEncoding { command, source })
}

#[cfg(test)]
mod tests;
