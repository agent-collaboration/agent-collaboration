use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{ArchitectureError, ItemEdge, ModuleEdge, ModuleGraph};

pub type PackageGraphs = BTreeMap<String, Vec<ModuleGraph>>;

#[derive(Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CycleBaseline(BTreeMap<String, Vec<FrozenComponent>>);

#[derive(Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct FrozenComponent {
    modules: Vec<String>,
    edges: Vec<ModuleEdge>,
}

impl CycleBaseline {
    pub fn from_json(json: &str) -> Result<Self, ArchitectureError> {
        let mut baseline: Self =
            serde_json::from_str(json).map_err(|source| ArchitectureError::Json {
                context: "decode module cycle baseline",
                source,
            })?;
        for (package, components) in &mut baseline.0 {
            let mut seen_modules = BTreeSet::new();
            for component in components.iter_mut() {
                if component.modules.len() < 2
                    || component
                        .modules
                        .iter()
                        .any(|module| module.is_empty() || !seen_modules.insert(module.clone()))
                {
                    return Err(ArchitectureError::InvalidBaseline(format!(
                        "{package}: SCCs need at least two distinct, nonempty, nonoverlapping modules"
                    )));
                }
                component.modules.sort();
                let mut seen_edges = BTreeSet::new();
                if component.edges.iter().any(|edge| {
                    edge.from.is_empty()
                        || edge.from == edge.to
                        || !component.modules.contains(&edge.to)
                        || !seen_edges.insert(edge.clone())
                }) {
                    return Err(ArchitectureError::InvalidBaseline(format!(
                        "{package}: edges must be distinct, cross-module, and point into their SCC"
                    )));
                }
                component.edges.sort();
            }
            components.sort();
        }
        Ok(baseline)
    }

    pub fn read(path: &Path) -> Result<Self, ArchitectureError> {
        let json = std::fs::read_to_string(path).map_err(|source| ArchitectureError::File {
            operation: "read baseline",
            path: path.to_owned(),
            source,
        })?;
        Self::from_json(&json)
    }

    pub fn from_graphs(packages: &PackageGraphs) -> Self {
        Self(
            packages
                .iter()
                .map(|(package, graphs)| {
                    let components: BTreeSet<_> = graphs
                        .iter()
                        .flat_map(|graph| {
                            graph
                                .cycle_components()
                                .into_iter()
                                .map(|modules| FrozenComponent {
                                    edges: graph.module_edges_into(&modules),
                                    modules,
                                })
                        })
                        .collect();
                    (package.clone(), components.into_iter().collect())
                })
                .collect(),
        )
    }

    pub fn write(&self, path: &Path) -> Result<(), ArchitectureError> {
        let encode_error = |source| ArchitectureError::Json {
            context: "encode module cycle baseline",
            source,
        };
        // Compact package rows keep even large edge sets within the repository's
        // file-line budget while making package-level baseline changes distinct.
        let mut lines = Vec::new();
        for (package, components) in &self.0 {
            let package_json = serde_json::to_string(package).map_err(encode_error)?;
            let components_json = serde_json::to_string(components).map_err(encode_error)?;
            lines.push(format!("  {package_json}: {components_json}"));
        }
        let json = format!("{{\n{}\n}}\n", lines.join(",\n"));
        std::fs::write(path, json).map_err(|source| ArchitectureError::File {
            operation: "write baseline",
            path: path.to_owned(),
            source,
        })
    }

    pub fn compare(&self, packages: &PackageGraphs) -> Result<(), RatchetViolation> {
        let observed = Self::from_graphs(packages);
        let mut changes = Vec::new();
        for (package, components) in &observed.0 {
            for component in components {
                let allowed = self
                    .0
                    .get(package)
                    .into_iter()
                    .flatten()
                    .find(|allowed| is_subset(&component.modules, &allowed.modules));
                let Some(allowed) = allowed else {
                    let item_edges = packages
                        .get(package)
                        .into_iter()
                        .flatten()
                        .flat_map(|graph| graph.cycle_item_edges(&component.modules))
                        .collect();
                    changes.push(CycleChange::Growth {
                        package: package.clone(),
                        modules: component.modules.clone(),
                        item_edges,
                    });
                    continue;
                };
                for edge in &component.edges {
                    if !allowed.edges.contains(edge) {
                        let item_edges = packages
                            .get(package)
                            .into_iter()
                            .flatten()
                            .flat_map(|graph| graph.item_edges_for(edge))
                            .collect();
                        changes.push(CycleChange::NewEdge {
                            package: package.clone(),
                            edge: edge.clone(),
                            item_edges,
                        });
                    }
                }
            }
        }
        for (package, components) in &self.0 {
            for component in components {
                let still_present = observed.0.get(package).is_some_and(|current| {
                    current.iter().any(|cycle| {
                        is_subset(&component.modules, &cycle.modules)
                            && component
                                .edges
                                .iter()
                                .all(|edge| cycle.edges.contains(edge))
                    })
                });
                if !still_present {
                    changes.push(CycleChange::Shrink {
                        package: package.clone(),
                        modules: component.modules.clone(),
                    });
                }
            }
        }
        if changes.is_empty() {
            Ok(())
        } else {
            Err(RatchetViolation { changes })
        }
    }
}

fn is_subset(left: &[String], right: &[String]) -> bool {
    left.iter().all(|module| right.contains(module))
}

#[derive(Debug)]
enum CycleChange {
    NewEdge {
        package: String,
        edge: ModuleEdge,
        item_edges: Vec<ItemEdge>,
    },
    Growth {
        package: String,
        modules: Vec<String>,
        item_edges: Vec<ItemEdge>,
    },
    Shrink {
        package: String,
        modules: Vec<String>,
    },
}

#[derive(Debug)]
pub struct RatchetViolation {
    changes: Vec<CycleChange>,
}

impl RatchetViolation {
    pub fn has_growth(&self) -> bool {
        self.changes.iter().any(|change| {
            matches!(
                change,
                CycleChange::Growth { .. } | CycleChange::NewEdge { .. }
            )
        })
    }
}

impl fmt::Display for RatchetViolation {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for change in &self.changes {
            let item_edges = match change {
                CycleChange::NewEdge {
                    package,
                    edge,
                    item_edges,
                } => {
                    writeln!(
                        output,
                        "{package}: new module edge inside or into SCC: {} -> {}",
                        edge.from, edge.to
                    )?;
                    item_edges
                }
                CycleChange::Growth {
                    package,
                    modules,
                    item_edges,
                } => {
                    writeln!(
                        output,
                        "{package}: new or enlarged SCC: {}",
                        modules.join(", ")
                    )?;
                    item_edges
                }
                CycleChange::Shrink { package, modules } => {
                    writeln!(
                        output,
                        "{package}: baseline SCC or its edges shrank or disappeared: {}; update the baseline (it may only shrink)",
                        modules.join(", ")
                    )?;
                    continue;
                }
            };
            for edge in item_edges {
                writeln!(output, "  {} -> {}", edge.from, edge.to)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for RatchetViolation {}

#[cfg(test)]
mod tests;
