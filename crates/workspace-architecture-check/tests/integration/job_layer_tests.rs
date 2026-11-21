//! Literal job-graph controls for direct dependencies and paths through lower crates.
use super::{
    DependencyKind, ENGINE_EDGES, FLEET_ENGINE, FLEET_SCHEDULES, FLEET_WAKES, TIMER_CRATES,
    check_boundaries, check_closed_workspace_edges, graph,
};

#[test]
fn timer_job_reachability_rejects_forbidden_edges_through_an_allowed_leaf() {
    let allowed = graph(&[
        (FLEET_WAKES, "workspace-model", DependencyKind::Normal),
        (
            "workspace-model",
            "collaboration-mail-protocol",
            DependencyKind::Normal,
        ),
    ]);
    assert!(check_boundaries(&allowed).is_ok());

    // Pin the required destinations independently of the rule's denylist.
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        for target in [
            "fleet-wakes",
            "fleet-mail",
            "collaboration-central-core",
            "collaboration-central-tokio",
            "collaboration-central-cloudflare",
            "codex-router-host",
            "codex-router-debug-hosts",
            "codex-router-tailnet-verification-host",
        ] {
            let indirect = graph(&[
                (FLEET_SCHEDULES, "agent-automation", DependencyKind::Normal),
                ("agent-automation", target, kind),
            ]);
            let failure = check_boundaries(&indirect)
                .expect_err("a timer must not reach a forbidden crate through an allowed leaf");
            assert_eq!(
                failure.to_string(),
                format!("BD1: forbidden reachable dependency {FLEET_SCHEDULES} -> {target}")
            );
        }
    }
}

#[test]
fn mail_rejects_sibling_and_host_dependencies_even_through_an_allowed_crate() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let pure = graph(&[
            ("fleet-mail", "workspace-model", DependencyKind::Normal),
            ("workspace-model", "collaboration-mail-protocol", kind),
        ]);
        assert!(check_boundaries(&pure).is_ok());

        for target in [
            "fleet-wakes",
            "fleet-schedules",
            "fleet-notices",
            "fleet-application",
            "collaboration-central-core",
            "collaboration-central-tokio",
            "collaboration-central-cloudflare",
            "codex-router-host",
            "codex-router-debug-hosts",
            "codex-router-tailnet-verification-host",
        ] {
            let direct = graph(&[("fleet-mail", target, kind)]);
            let failure =
                check_boundaries(&direct).expect_err("Mail has a closed direct allowlist");
            assert_eq!(
                failure.to_string(),
                format!("BD1: forbidden {kind:?} dependency fleet-mail -> {target}")
            );

            let indirect = graph(&[
                (
                    "fleet-mail",
                    "fleet-workspace-access",
                    DependencyKind::Normal,
                ),
                ("fleet-workspace-access", target, kind),
            ]);
            let failure = check_boundaries(&indirect)
                .expect_err("Mail cannot reach a job or host indirectly");
            assert_eq!(
                failure.to_string(),
                format!("BD1: forbidden reachable dependency fleet-mail -> {target}")
            );
        }
    }
}

#[test]
fn mail_rejects_timer_policy_through_workspace_model_and_automation() {
    let outcomes = [DependencyKind::Normal, DependencyKind::Build].map(|kind| {
        let indirect = graph(&[
            ("fleet-mail", "workspace-model", DependencyKind::Normal),
            (
                "workspace-model",
                "agent-automation",
                DependencyKind::Normal,
            ),
            ("agent-automation", "fleet-timer-policy", kind),
        ]);
        check_boundaries(&indirect).map_err(|failure| failure.to_string())
    });
    let forbidden = "BD1: forbidden reachable dependency fleet-mail -> fleet-timer-policy";
    assert_eq!(
        outcomes,
        [Err(forbidden.to_owned()), Err(forbidden.to_owned())]
    );
}

#[test]
fn each_timer_rejects_a_path_through_an_allowed_crate_into_mail() {
    for timer in ["fleet-wakes", "fleet-schedules"] {
        for kind in [DependencyKind::Normal, DependencyKind::Build] {
            let indirect = graph(&[
                (timer, "fleet-engine", DependencyKind::Normal),
                ("fleet-engine", "fleet-runtime", DependencyKind::Normal),
                ("fleet-runtime", "fleet-mail", kind),
            ]);
            let failure = check_boundaries(&indirect).expect_err("a timer must not reach Mail");
            assert_eq!(
                failure.to_string(),
                format!("BD1: forbidden reachable dependency {timer} -> fleet-mail")
            );
        }
    }
}

#[test]
fn engine_rejects_core_job_and_host_edges() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        assert!(
            check_closed_workspace_edges(
                &graph(&[(FLEET_ENGINE, "fleet-runtime", kind)]),
                FLEET_ENGINE,
                ENGINE_EDGES
            )
            .is_ok()
        );
        for target in [
            "collaboration-central-core",
            "fleet-wakes",
            "fleet-mail",
            "fleet-notices",
            "collaboration-central-cloudflare",
        ] {
            let error = check_closed_workspace_edges(
                &graph(&[(FLEET_ENGINE, target, kind)]),
                FLEET_ENGINE,
                ENGINE_EDGES,
            )
            .expect_err("engine dependencies must point down");
            assert_eq!(
                error.to_string(),
                format!("BD1: forbidden {kind:?} dependency {FLEET_ENGINE} -> {target}")
            );
        }
    }
}

#[test]
fn timer_crates_reject_upward_and_sibling_edges() {
    for (package, allowed) in TIMER_CRATES {
        for kind in [DependencyKind::Normal, DependencyKind::Build] {
            assert!(
                check_closed_workspace_edges(
                    &graph(&[(package, "central-engine-model", kind)]),
                    package,
                    allowed
                )
                .is_ok()
            );
            for target in [
                FLEET_WAKES,
                FLEET_SCHEDULES,
                "fleet-mail",
                "collaboration-central-core",
                "collaboration-central-cloudflare",
            ] {
                if target == package {
                    continue;
                }
                let error = check_closed_workspace_edges(
                    &graph(&[(package, target, kind)]),
                    package,
                    allowed,
                )
                .expect_err("timer dependencies must point down");
                assert_eq!(
                    error.to_string(),
                    format!("BD1: forbidden {kind:?} dependency {package} -> {target}")
                );
            }
        }
    }
}

#[test]
fn notices_rejects_siblings_and_hosts_through_workspace_access() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let pure = graph(&[
            ("fleet-notices", "workspace-model", DependencyKind::Normal),
            ("workspace-model", "collaboration-mail-protocol", kind),
        ]);
        assert!(check_boundaries(&pure).is_ok());

        for target in [
            "fleet-mail",
            "fleet-wakes",
            "fleet-schedules",
            "fleet-timer-policy",
            "fleet-application",
            "collaboration-central-core",
            "collaboration-central-tokio",
            "collaboration-central-cloudflare",
            "codex-router-host",
            "codex-router-debug-hosts",
            "codex-router-tailnet-verification-host",
        ] {
            let direct = graph(&[("fleet-notices", target, kind)]);
            assert_eq!(
                check_boundaries(&direct).map_err(|failure| failure.to_string()),
                Err(format!(
                    "BD1: forbidden {kind:?} dependency fleet-notices -> {target}"
                )),
            );
            let indirect = graph(&[
                (
                    "fleet-notices",
                    "fleet-workspace-access",
                    DependencyKind::Normal,
                ),
                ("fleet-workspace-access", target, kind),
            ]);
            assert_eq!(
                check_boundaries(&indirect).map_err(|failure| failure.to_string()),
                Err(format!(
                    "BD1: forbidden reachable dependency fleet-notices -> {target}"
                )),
            );
        }
    }
}

#[test]
fn each_timer_rejects_a_path_through_runtime_into_notices() {
    for timer in ["fleet-wakes", "fleet-schedules"] {
        for kind in [DependencyKind::Normal, DependencyKind::Build] {
            let indirect = graph(&[
                (timer, "fleet-engine", DependencyKind::Normal),
                ("fleet-engine", "fleet-runtime", DependencyKind::Normal),
                ("fleet-runtime", "fleet-notices", kind),
            ]);
            assert_eq!(
                check_boundaries(&indirect).map_err(|failure| failure.to_string()),
                Err(format!(
                    "BD1: forbidden reachable dependency {timer} -> fleet-notices"
                )),
            );
        }
    }
}
