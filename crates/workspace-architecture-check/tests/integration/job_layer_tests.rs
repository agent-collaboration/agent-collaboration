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

    // Pin the forbidden sibling independently of the rule's denylist.
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let target = "fleet-wakes";
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

#[test]
fn schedules_rejects_a_transitive_host_dependency() {
    let outcomes = [DependencyKind::Normal, DependencyKind::Build].map(|kind| {
        let indirect = graph(&[
            (FLEET_SCHEDULES, "agent-automation", DependencyKind::Normal),
            ("agent-automation", "codex-router-host", kind),
        ]);
        check_boundaries(&indirect).map_err(|failure| failure.to_string())
    });
    let forbidden = "BD1: forbidden reachable dependency fleet-schedules -> codex-router-host";
    assert_eq!(
        outcomes,
        [Err(forbidden.to_owned()), Err(forbidden.to_owned())]
    );
}

#[test]
fn mail_rejects_a_direct_sibling_dependency() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let pure = graph(&[
            ("fleet-mail", "workspace-model", DependencyKind::Normal),
            ("workspace-model", "collaboration-mail-protocol", kind),
        ]);
        assert!(check_boundaries(&pure).is_ok());

        let target = "fleet-wakes";
        let direct = graph(&[("fleet-mail", target, kind)]);
        let failure = check_boundaries(&direct).expect_err("Mail has a closed direct allowlist");
        assert_eq!(
            failure.to_string(),
            format!("BD1: forbidden {kind:?} dependency fleet-mail -> {target}")
        );
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
fn engine_rejects_an_upward_core_dependency() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        assert!(
            check_closed_workspace_edges(
                &graph(&[(FLEET_ENGINE, "fleet-runtime", kind)]),
                FLEET_ENGINE,
                ENGINE_EDGES
            )
            .is_ok()
        );
        let target = "collaboration-central-core";
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

#[test]
fn timer_crates_reject_a_direct_mail_dependency() {
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
            let target = "fleet-mail";
            let error =
                check_closed_workspace_edges(&graph(&[(package, target, kind)]), package, allowed)
                    .expect_err("timer dependencies must point down");
            assert_eq!(
                error.to_string(),
                format!("BD1: forbidden {kind:?} dependency {package} -> {target}")
            );
        }
    }
}

#[test]
fn notices_rejects_direct_and_transitive_mail_dependencies() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let pure = graph(&[
            ("fleet-notices", "workspace-model", DependencyKind::Normal),
            ("workspace-model", "collaboration-mail-protocol", kind),
        ]);
        assert!(check_boundaries(&pure).is_ok());

        let target = "fleet-mail";
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

#[test]
fn wakes_rejects_a_path_through_runtime_into_notices() {
    let timer = "fleet-wakes";
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

#[test]
fn directory_rejects_upper_dependencies_through_its_board_leaf() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let allowed = graph(&[
            ("fleet-workspace-directory", "message-board", kind),
            ("fleet-workspace-directory", "fleet-contracts", kind),
            ("fleet-workspace-directory", "fleet-runtime-ports", kind),
            ("fleet-workspace-directory", "central-storage", kind),
            ("fleet-workspace-directory", "workspace-model", kind),
        ]);
        assert!(check_boundaries(&allowed).is_ok());

        let upper = "fleet-engine";
        let direct = graph(&[("fleet-workspace-directory", upper, kind)]);
        assert_eq!(
            check_boundaries(&direct).map_err(|failure| failure.to_string()),
            Err(format!(
                "BD1: forbidden {kind:?} dependency fleet-workspace-directory -> {upper}"
            )),
        );
        let transitive = graph(&[
            (
                "fleet-workspace-directory",
                "message-board",
                DependencyKind::Normal,
            ),
            ("message-board", upper, kind),
        ]);
        assert_eq!(
            check_boundaries(&transitive).map_err(|failure| failure.to_string()),
            Err(format!(
                "BD1: forbidden reachable dependency fleet-workspace-directory -> {upper}"
            )),
        );
    }
}

#[test]
fn engine_rejects_a_transitive_directory_dependency() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let engine = graph(&[
            ("fleet-engine", "fleet-runtime", DependencyKind::Normal),
            ("fleet-runtime", "fleet-workspace-directory", kind),
        ]);
        assert_eq!(
            check_boundaries(&engine).map_err(|failure| failure.to_string()),
            Err(
                "BD1: forbidden reachable dependency fleet-engine -> fleet-workspace-directory"
                    .into()
            ),
        );
    }
}
