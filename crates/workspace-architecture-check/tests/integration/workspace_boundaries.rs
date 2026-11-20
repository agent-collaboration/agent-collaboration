//! BD1's closed workspace dependencies and BD4's transitive proxy boundary.
use super::{DependencyKind, NonDevDependencyGraph};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub(super) const FLEET_PURE_CRATES: [(&str, &[&str]); 2] = [
    (
        "fleet-runtime-ports",
        &["central-engine-model", "central-storage", "workspace-ids"],
    ),
    (
        "fleet-contracts",
        &[
            "central-engine-model",
            "central-storage",
            "fleet-identity-protocol",
            "fleet-runtime-ports",
            "message-board",
            "workspace-ids",
            "workspace-model",
        ],
    ),
];
const FLEET_WASM_TARGET: &str = "wasm32-unknown-unknown";
// Chrono's wasm support reaches these runtime-independent helpers via js-sys.
const FLEET_WASM_SUPPORT_PACKAGES: &[&str] = &["futures-core", "futures-task", "futures-util"];

const PURE_CRATES: [(&str, &[&str]); 7] = [
    ("workspace-ids", &[]),
    (
        "workspace-model",
        &[
            "workspace-ids",
            "agent-automation",
            // The delivery frame reuses the pure session and machine label contracts.
            "collaboration-mail-protocol",
            "message-board",
        ],
    ),
    (
        "central-link-protocol",
        &["workspace-ids", "workspace-model"],
    ),
    ("central-storage", &[]),
    ("central-engine-model", &["central-storage"]),
    ("workspace-rules", &["workspace-model", "central-storage"]),
    ("vendor-protocol", &[]),
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
        || matches!(name, "agent-collaboration" | "agent-automation")
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
    for name in [
        "collaboration-central-core",
        "collaboration-central-cloudflare",
        "collaboration-central-tokio",
    ] {
        let Some(source) = graph.workspace_id(name) else {
            continue;
        };
        for (destination, _) in graph.edges.get(source).into_iter().flatten() {
            if graph
                .names
                .get(destination)
                .is_some_and(|target| target == "collaboration-protocol")
            {
                return Err(format!(
                    "C1: {name} retains a legacy collaboration-protocol dependency"
                )
                .into());
            }
        }
    }
    for (name, allowed) in PURE_CRATES.into_iter().chain(FLEET_PURE_CRATES) {
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
        check_workspace_driver_edges(graph, source)?;
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

// Workspace-wide metadata merges unrelated features into external packages.
// Follow workspace edges here; the package-scoped trees below check full closures.
fn check_workspace_driver_edges(
    graph: &NonDevDependencyGraph,
    source: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![source];
    while let Some(current) = pending.pop() {
        if !seen.insert(current) {
            continue;
        }
        for (destination, _) in graph.edges.get(current).into_iter().flatten() {
            let target = graph
                .names
                .get(destination)
                .ok_or("dependency name absent")?;
            if is_effectful(target) {
                return Err(format!("BD2: pure workspace dependency reaches {target}").into());
            }
            if graph.workspace_members.contains(destination) {
                pending.push(destination);
            }
        }
    }
    Ok(())
}

/// Workspace metadata merges features from unrelated roots (notably uuid's wasm
/// support). A package-scoped tree checks the actual pure-rule dependency closure.
pub(super) fn check_pure_dependency_tree(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for (package, _) in PURE_CRATES.into_iter().chain(FLEET_PURE_CRATES) {
        check_package_targets(root, package)?;
    }
    Ok(())
}

fn check_package_targets(root: &Path, package: &str) -> Result<(), Box<dyn std::error::Error>> {
    check_package_tree(root, package, None)?;
    if FLEET_PURE_CRATES.iter().any(|(name, _)| *name == package) {
        check_package_tree(root, package, Some(FLEET_WASM_TARGET)).map_err(|error| {
            format!("{package} dependency tree for {FLEET_WASM_TARGET}: {error}")
        })?;
    }
    Ok(())
}

fn check_package_tree(
    root: &Path,
    package: &str,
    target: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cargo = Command::new("cargo");
    cargo.args([
        "tree",
        "--locked",
        "--offline",
        "-p",
        package,
        "-e",
        "normal,build",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]);
    if let Some(triple) = target {
        cargo.args(["--target", triple]);
    }
    let output = cargo.current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!(
            "pure dependency tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let allowed_support = if target == Some(FLEET_WASM_TARGET) {
        FLEET_WASM_SUPPORT_PACKAGES
    } else {
        &[]
    };
    check_tree_output(&String::from_utf8(output.stdout)?, allowed_support)
}
fn check_tree_output(
    tree: &str,
    allowed_support: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    for name in tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
    {
        if is_effectful(name) && !allowed_support.contains(&name) {
            return Err(format!("BD2: pure rules transitively reach {name}").into());
        }
    }
    Ok(())
}

#[test]
fn pure_package_tree_rejects_effectful_transitive_dependencies() {
    assert!(check_tree_output("workspace-rules v1\nserde v1\ntokio v1", &[]).is_err());
    assert!(check_tree_output("workspace-rules v1\nsqlx-sqlite v1", &[]).is_err());
    assert!(
        check_tree_output(
            "workspace-rules v1\nworkspace-model v1\nuuid v1\nserde v1",
            &[]
        )
        .is_ok()
    );
}

#[test]
fn fleet_package_tree_rejects_an_indirect_wasm_worker_dependency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-worker-dependency");
    for package in ["fleet-contracts", "fleet-runtime-ports"] {
        check_package_tree(&root, package, None).expect("the native dependency tree is pure");

        let violation = check_package_targets(&root, package)
            .expect_err("the Fleet package must reject the indirect wasm-only Worker dependency");

        assert_eq!(
            violation.to_string(),
            format!(
                "{package} dependency tree for wasm32-unknown-unknown: BD2: pure rules transitively reach worker"
            )
        );
    }
}

#[test]
fn mail_contract_dependency_keeps_model_and_link_transitively_driver_free() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        assert!(check_boundaries(&graph(&[("workspace-model", "agent-automation", kind)])).is_ok());
        assert!(
            check_boundaries(&graph(&[
                ("workspace-model", "collaboration-mail-protocol", kind),
                ("workspace-model", "message-board", kind),
            ]))
            .is_ok()
        );
        assert!(
            check_boundaries(&graph(&[
                ("workspace-model", "agent-automation", kind),
                ("agent-automation", "tokio", kind),
            ]))
            .is_err()
        );
        assert!(
            check_boundaries(&graph(&[
                ("central-link-protocol", "workspace-model", kind),
                ("workspace-model", "agent-automation", kind),
                ("agent-automation", "sqlx-sqlite", kind),
            ]))
            .is_err()
        );
        assert!(check_boundaries(&graph(&[("workspace-model", "unlisted-host", kind)])).is_err());
    }
}

#[test]
fn central_owners_reject_direct_legacy_protocol_edges() {
    for owner in [
        "collaboration-central-core",
        "collaboration-central-cloudflare",
        "collaboration-central-tokio",
    ] {
        for kind in [DependencyKind::Normal, DependencyKind::Build] {
            assert!(check_boundaries(&graph(&[(owner, "collaboration-protocol", kind)])).is_err());
        }
        assert!(
            check_boundaries(&graph(&[(
                owner,
                "collaboration-protocol",
                DependencyKind::Dev
            )]))
            .is_ok()
        );
    }
}

#[test]
fn fleet_interfaces_reject_upward_normal_and_build_edges() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        assert!(
            check_boundaries(&graph(&[("fleet-contracts", "fleet-runtime-ports", kind)])).is_ok()
        );
    }
    for interface in ["fleet-runtime-ports", "fleet-contracts"] {
        for owner in [
            "collaboration-central-core",
            "collaboration-central-tokio",
            "collaboration-central-cloudflare",
        ] {
            for kind in [DependencyKind::Normal, DependencyKind::Build] {
                let violation = check_boundaries(&graph(&[(interface, owner, kind)]))
                    .expect_err("the Fleet interface must reject an upward edge");
                assert_eq!(
                    violation.to_string(),
                    format!("BD1: forbidden {kind:?} dependency {interface} -> {owner}")
                );
            }
        }
    }
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
