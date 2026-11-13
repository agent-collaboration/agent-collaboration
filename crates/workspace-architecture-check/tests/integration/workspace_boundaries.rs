//! BD1's closed workspace dependencies and BD4's transitive proxy boundary.
use super::{DependencyKind, NonDevDependencyGraph};
use std::collections::{BTreeMap, BTreeSet};

const PURE_CRATES: [(&str, &[&str]); 4] = [
    ("workspace-ids", &[]),
    ("workspace-model", &["workspace-ids"]),
    ("central-storage", &[]),
    ("workspace-rules", &["workspace-model", "central-storage"]),
];
const PROXY_CRATES: &[&str] = &[
    "codex-router-proxy",
    "codex-router-state",
    "codex-router-quota",
    "codex-router-selection",
    "codex-router-auth",
    "codex-router-quota-service",
];
fn is_collaboration(name: &str) -> bool {
    name.starts_with("collaboration-")
        || name.starts_with("message-board")
        || name.starts_with("board-")
        || name.starts_with("fleet-")
        || name.starts_with("workspace-")
        || name.starts_with("central-")
        || matches!(
            name,
            "agent-collaboration" | "agent-automation" | "automation-storage"
        )
}
fn is_effectful(name: &str) -> bool {
    name.starts_with("tokio")
        || name.starts_with("futures")
        || name.starts_with("async-")
        || name.starts_with("sqlx")
        || name.starts_with("turso")
        || name.starts_with("rusqlite")
        || name.starts_with("worker")
        || name.starts_with("reqwest")
        || name.starts_with("hyper")
        || matches!(
            name,
            "smol" | "mio" | "durable-steps" | "runtime-primitives" | "router-clock"
        )
}
fn reachable<'a>(graph: &'a NonDevDependencyGraph, source: &'a str) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![source];
    while let Some(current) = pending.pop() {
        for (destination, _) in graph.edges.get(current).into_iter().flatten() {
            if seen.insert(destination.as_str()) {
                pending.push(destination);
            }
        }
    }
    seen
}
pub(super) fn check_boundaries(
    graph: &NonDevDependencyGraph,
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, allowed) in PURE_CRATES {
        let Some(source) = graph.workspace_id(name) else {
            continue;
        };
        for (destination, kind) in graph.edges.get(source).into_iter().flatten() {
            let target = graph
                .names
                .get(destination)
                .ok_or("dependency name absent")?;
            if is_effectful(target) {
                return Err(format!("BD2: {name} depends on effectful package {target}").into());
            }
            if graph.workspace_members.contains(destination) && !allowed.contains(&target.as_str())
            {
                return Err(
                    format!("BD1: forbidden {kind:?} dependency {name} -> {target}").into(),
                );
            }
        }
    }
    for name in PROXY_CRATES {
        let Some(source) = graph.workspace_id(name) else {
            continue;
        };
        for destination in reachable(graph, source) {
            let target = graph
                .names
                .get(destination)
                .ok_or("dependency name absent")?;
            if is_collaboration(target) {
                return Err(
                    format!("BD4: {name} reaches collaboration dependency {target}").into(),
                );
            }
        }
    }
    Ok(())
}

/// Workspace metadata merges features from unrelated roots (notably uuid's wasm
/// support). A package-scoped tree checks the actual pure-rule dependency closure.
pub(super) fn check_pure_dependency_tree(
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new("cargo")
        .args([
            "tree",
            "--locked",
            "--offline",
            "-p",
            "workspace-rules",
            "-e",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "pure dependency tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    check_tree_output(&String::from_utf8(output.stdout)?)
}
fn check_tree_output(tree: &str) -> Result<(), Box<dyn std::error::Error>> {
    for name in tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
    {
        if is_effectful(name) {
            return Err(format!("BD2: pure rules transitively reach {name}").into());
        }
    }
    Ok(())
}

#[test]
fn pure_package_tree_rejects_effectful_transitive_dependencies() {
    assert!(check_tree_output("workspace-rules v1\nserde v1\ntokio v1").is_err());
    assert!(check_tree_output("workspace-rules v1\nsqlx-sqlite v1").is_err());
    assert!(check_tree_output("workspace-rules v1\nworkspace-model v1\nuuid v1\nserde v1").is_ok());
}

fn graph(edges: &[(&str, &str, DependencyKind)]) -> NonDevDependencyGraph {
    let names: BTreeMap<_, _> = edges
        .iter()
        .flat_map(|(from, to, _)| [*from, *to])
        .map(|name| (name.to_owned(), name.to_owned()))
        .collect();
    let mut graph = NonDevDependencyGraph {
        workspace_members: names
            .keys()
            .filter(|name| !matches!(name.as_str(), "tokio" | "sqlx-sqlite"))
            .cloned()
            .collect(),
        names,
        edges: BTreeMap::new(),
        dev_edges: BTreeMap::new(),
    };
    for (from, to, kind) in edges {
        match kind {
            DependencyKind::Dev => {
                graph
                    .dev_edges
                    .entry((*from).to_owned())
                    .or_default()
                    .insert((*to).to_owned());
            }
            DependencyKind::Normal | DependencyKind::Build => {
                graph
                    .edges
                    .entry((*from).to_owned())
                    .or_default()
                    .insert(((*to).to_owned(), *kind));
            }
        }
    }
    graph
}
#[test]
fn pure_rules_reject_unlisted_workspace_and_transitive_external_effects() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        assert!(check_boundaries(&graph(&[("workspace-rules", "unlisted-host", kind)])).is_err());
        assert!(
            check_boundaries(&graph(&[
                ("workspace-rules", "workspace-model", kind),
                ("workspace-model", "tokio", kind)
            ]))
            .is_err()
        );
        assert!(check_boundaries(&graph(&[("workspace-rules", "sqlx-sqlite", kind)])).is_err());
    }
    assert!(
        check_boundaries(&graph(&[
            ("workspace-rules", "workspace-model", DependencyKind::Normal),
            ("workspace-model", "workspace-ids", DependencyKind::Build)
        ]))
        .is_ok()
    );
}
#[test]
fn proxy_boundary_rejects_direct_and_transitive_normal_and_build_edges() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        assert!(
            check_boundaries(&graph(&[(
                "codex-router-proxy",
                "collaboration-runtime",
                kind
            )]))
            .is_err()
        );
        assert!(
            check_boundaries(&graph(&[
                ("codex-router-auth", "unlisted-middle", kind),
                ("unlisted-middle", "message-board", kind)
            ]))
            .is_err()
        );
    }
    assert!(
        check_boundaries(&graph(&[(
            "codex-router-host",
            "collaboration-runtime",
            DependencyKind::Normal
        )]))
        .is_ok()
    );
    assert!(
        check_boundaries(&graph(&[(
            "codex-router-proxy",
            "collaboration-runtime",
            DependencyKind::Dev
        )]))
        .is_ok()
    );
}
