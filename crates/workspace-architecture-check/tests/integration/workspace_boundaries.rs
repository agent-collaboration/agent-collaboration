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
const FLEET_ACCESS: &str = "fleet-workspace-access";
const FLEET_ENGINE: &str = "fleet-engine";
pub(super) const ENGINE_EDGES: &[&str] = &[
    "central-engine-model",
    "central-storage",
    "fleet-contracts",
    "fleet-runtime",
    "fleet-runtime-ports",
    "workspace-model",
];
const FLEET_ENGINE_FUTURES: &[&str] = &[
    "futures-core",
    "futures-sink",
    "futures-task",
    "futures-util",
];
const FLEET_WASM_TARGET: &str = "wasm32-unknown-unknown";
// Chrono's wasm support reaches these runtime-independent helpers via js-sys.
const FLEET_WASM_SUPPORT_PACKAGES: &[&str] = &["futures-core", "futures-task", "futures-util"];

const PURE_CRATES: [(&str, &[&str]); 8] = [
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
    ("state-machine-core", &[]),
    (
        "central-engine-model",
        &["central-storage", "state-machine-core"],
    ),
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
    check_engine_workspace_edges(graph)?;
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

fn check_engine_workspace_edges(
    graph: &NonDevDependencyGraph,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(source) = graph.workspace_id(FLEET_ENGINE) else {
        return Ok(());
    };
    for (destination, kind) in graph.edges.get(source).into_iter().flatten() {
        if graph.workspace_members.contains(destination) {
            let target = graph
                .names
                .get(destination)
                .ok_or("dependency name absent")?;
            if !ENGINE_EDGES.contains(&target.as_str()) {
                return Err(format!(
                    "BD1: forbidden {kind:?} dependency {FLEET_ENGINE} -> {target}"
                )
                .into());
            }
        }
    }
    Ok(())
}

#[test]
fn engine_rejects_core_job_and_host_edges() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        assert!(
            check_engine_workspace_edges(&graph(&[(FLEET_ENGINE, "fleet-runtime", kind)])).is_ok()
        );
        for target in [
            "collaboration-central-core",
            "fleet-wakes",
            "collaboration-central-cloudflare",
        ] {
            let error = check_engine_workspace_edges(&graph(&[(FLEET_ENGINE, target, kind)]))
                .expect_err("engine dependencies must point down");
            assert_eq!(
                error.to_string(),
                format!("BD1: forbidden {kind:?} dependency {FLEET_ENGINE} -> {target}")
            );
        }
    }
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
    for package in PURE_CRATES
        .into_iter()
        .chain(FLEET_PURE_CRATES)
        .map(|(package, _)| package)
        .chain([FLEET_ACCESS, FLEET_ENGINE])
    {
        check_package_targets(root, package)?;
    }
    Ok(())
}

fn check_package_targets(root: &Path, package: &str) -> Result<(), Box<dyn std::error::Error>> {
    check_package_tree(root, package, None)?;
    if matches!(package, FLEET_ACCESS | FLEET_ENGINE)
        || FLEET_PURE_CRATES.iter().any(|(name, _)| *name == package)
    {
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
        "{p}\t{f}",
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
    let allowed_support = if package == FLEET_ENGINE {
        FLEET_ENGINE_FUTURES
    } else if target == Some(FLEET_WASM_TARGET) {
        FLEET_WASM_SUPPORT_PACKAGES
    } else {
        &[]
    };
    check_tree_output(&String::from_utf8(output.stdout)?, allowed_support, package)
}
fn check_tree_output(
    tree: &str,
    allowed_support: &[&str],
    package: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for line in tree.lines() {
        let (identity, features) = line.split_once('\t').unwrap_or((line, ""));
        let Some(name) = identity.split_whitespace().next() else {
            continue;
        };
        if !is_effectful(name) || allowed_support.contains(&name) {
            continue;
        }
        if matches!(package, FLEET_ACCESS | FLEET_ENGINE) && name == "tokio" {
            let features = features.strip_suffix(" (*)").unwrap_or(features).trim();
            if matches!(features, "" | "sync") {
                continue;
            }
            return Err(format!(
                "BD2: {package} reaches Tokio features {features}; only sync is allowed"
            )
            .into());
        }
        return Err(format!("BD2: pure rules transitively reach {name}").into());
    }
    Ok(())
}

#[test]
fn pure_package_tree_rejects_effectful_transitive_dependencies() {
    assert!(
        check_tree_output(
            "workspace-rules v1\nserde v1\ntokio v1",
            &[],
            "workspace-rules"
        )
        .is_err()
    );
    assert!(
        check_tree_output("workspace-rules v1\nsqlx-sqlite v1", &[], "workspace-rules").is_err()
    );
    assert!(
        check_tree_output(
            "workspace-rules v1\nworkspace-model v1\nuuid v1\nserde v1",
            &[],
            "workspace-rules",
        )
        .is_ok()
    );
}

#[test]
fn access_dependency_tree_allows_tokio_sync_but_rejects_the_executor() {
    let valid = "fleet-workspace-access v1\t\ntokio v1\tsync";
    assert!(check_tree_output(valid, &[], FLEET_ACCESS).is_ok());

    let invalid = "fleet-workspace-access v1\t\ntokio v1\trt,sync";
    let violation = check_tree_output(invalid, &[], FLEET_ACCESS)
        .expect_err("workspace access cannot enable the Tokio executor");
    assert_eq!(
        violation.to_string(),
        "BD2: fleet-workspace-access reaches Tokio features rt,sync; only sync is allowed"
    );
}

#[test]
fn fleet_package_tree_rejects_an_indirect_wasm_worker_dependency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-worker-dependency");
    for package in ["fleet-contracts", "fleet-runtime-ports", FLEET_ACCESS] {
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
