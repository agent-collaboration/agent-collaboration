use codex_router_host::AppServerCondition;
use codex_router_host::ExecutableRelation;
use codex_router_host::HostOperation;
use codex_router_host::HostPhase;
use codex_router_host::HostSnapshotDimensions;
use codex_router_host::HostTerminalResponse;
use codex_router_host::LifecycleOutcome;
use codex_router_host::LifecycleOutcomeClassification;
use codex_router_host::OperatorRequest;
use codex_router_host::RecoveryBudget;
use codex_router_host::RemoteControlCondition;
use codex_router_host::RouterCondition;
use codex_router_host::TerminalClassification;

use super::*;

#[test]
fn central_refusal_is_named_in_readiness_without_changing_local_readiness() {
    use serde_json::json;
    for (remote_control, classification, baseline) in [
        (
            RemoteControlCondition::Connected,
            TerminalClassification::Ready,
            "ready",
        ),
        (
            RemoteControlCondition::Unavailable,
            TerminalClassification::LocalReadyRemoteDegraded,
            "local ready (Remote Control degraded)",
        ),
    ] {
        let snapshot = HostSnapshot::new(HostSnapshotDimensions {
            phase: HostPhase::Steady,
            router: RouterCondition::ExternalReachable,
            app_server: AppServerCondition::NativeReady {
                running_version: "1.2.3".to_owned(),
            },
            remote_control,
            remote_control_identity: None,
            executable_relation: ExecutableRelation::Match,
            router_executable_relation: codex_router_host::RouterExecutableRelation::Match,
            recovery_budget: RecoveryBudget::Available,
            last_lifecycle_outcome: None,
        });
        let readiness = snapshot.hosted_readiness();
        for (state, refusal) in [
            (
                json!({"kind":"refused","reason":"identityInUseElsewhere","at":"2026-11-06T12:00:00Z"}),
                Some("IdentityInUseElsewhere"),
            ),
            (
                json!({"kind":"refused","reason":{"credentialRejected":{"reason":"unknown"}},"at":"2026-11-06T12:00:00Z"}),
                Some("CredentialRejected { reason: Unknown }"),
            ),
            (
                json!({"kind":"connected","since":"2026-11-06T12:00:00Z","lastContact":"2026-11-06T12:00:20Z"}),
                None,
            ),
            (
                json!({"kind":"connecting","since":"2026-11-06T12:00:00Z","attempts":2}),
                None,
            ),
            (
                json!({"kind":"disconnected","since":"2026-11-06T12:00:00Z"}),
                None,
            ),
        ] {
            // Arrange: decode the actual operator snapshot shape, with one link
            // observation shared by the central line and both readiness summaries.
            let mut wire = serde_json::to_value(&snapshot).unwrap();
            wire["fleet"] = json!({"kind":"available","central":{
                "peer":"019f0000-0000-7000-8000-000000008131",
                "endpoint":"wss://central.example/session","instance":0,"connection":1,"revision":2,
                "state":state
            },"stuckMail":{"kind":"empty"}});
            let actual: HostSnapshot = serde_json::from_value(wire).unwrap();
            let mut output = Vec::new();

            // Act: use the complete status presenter that operators consume.
            render_terminal_frame(
                &mut output,
                &[OperatorFrame::terminal(HostTerminalResponse::new(
                    OperatorRequest::Status,
                    classification,
                    actual.clone(),
                    "status".to_owned(),
                ))],
            )
            .unwrap();

            // Assert: refusal is visible in both summaries; transient states keep
            // their text and no presentation decision changes ready-gating state.
            let rendered = String::from_utf8(output).unwrap();
            let expected = refusal.map_or_else(
                || baseline.to_owned(),
                |reason| {
                    if remote_control == RemoteControlCondition::Connected {
                        format!("local ready (central refused: {reason})")
                    } else {
                        format!("local ready (Remote Control degraded, central refused: {reason})")
                    }
                },
            );
            for field in ["result", "readiness"] {
                assert!(
                    rendered
                        .lines()
                        .any(|line| line == format!("{field}: {expected}")),
                    "{rendered}"
                );
            }
            assert!(rendered.lines().any(|line| line == "phase: steady"));
            assert_eq!(actual.hosted_readiness(), readiness);
        }
    }
}

#[test]
fn central_status_shows_contact_drop_and_stuck_mail_age() {
    let central: collaboration_protocol::FleetLinkRecord = serde_json::from_value(serde_json::json!({
        "peer":"019f0000-0000-7000-8000-000000008131", "endpoint":"wss://central.example/session",
        "instance":0,"connection":0,"revision":0,"lastFrameAt":"2026-11-06T12:00:20Z",
        "state":{"kind":"connected","since":"2026-11-06T12:00:00Z","lastContact":"2026-11-06T12:00:20Z"},
        "lastDrop":{"reason":"idleTimeout","at":"2026-11-06T11:59:00Z","connection":0}
    })).unwrap();
    let observation = codex_router_host::FleetHostObservation::Available {
        central: Some(Box::new(central)),
        stuck_mail: collaboration_protocol::StuckMailSummary::Stuck {
            count: std::num::NonZeroU64::new(2).unwrap(),
            oldest_age_seconds: 601,
        },
    };
    let mut output = Vec::new();
    render_fleet_status(&mut output, &observation).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("central: connected since 2026-11-06 12:00:00 UTC; last message from central 2026-11-06 12:00:20 UTC; last drop idleTimeout at 2026-11-06 11:59:00 UTC"));
    assert!(output.contains("outbox: 2 items sent longer than 5 minutes; oldest age 601s"));
    assert!(
        output.contains("instance 0; connection 0; revision 0"),
        "{output}"
    );
}

#[test]
fn recovered_sent_age_and_mail_read_errors_preserve_the_central_line() {
    let central: collaboration_protocol::FleetLinkRecord = serde_json::from_value(serde_json::json!({
        "peer":"019f0000-0000-7000-8000-000000008131", "endpoint":"wss://central.example/session",
        "instance":0,"connection":0,"revision":0,"lastFrameAt":"2026-11-06T12:00:20Z",
        "state":{"kind":"connected","since":"2026-11-06T12:00:00Z","lastContact":"2026-11-06T12:00:20Z"},
        "lastDrop":null
    })).unwrap();
    for known_stuck in [
        None,
        Some(collaboration_protocol::KnownStuckMail {
            count: std::num::NonZeroU64::MIN,
            oldest_age_seconds: 601,
        }),
    ] {
        let has_known_age = known_stuck.is_some();
        let observation = codex_router_host::FleetHostObservation::Available {
            central: Some(Box::new(central.clone())),
            stuck_mail: collaboration_protocol::StuckMailSummary::AgeUnknown {
                recovered_count: std::num::NonZeroU64::MIN,
                known_stuck,
            },
        };
        let mut output = Vec::new();
        render_fleet_status(&mut output, &observation).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("central: connected"));
        assert!(output.contains("1 outstanding sent items; age unknown (recovered)"));
        assert!(!output.contains("outbox: 0"));
        if has_known_age {
            assert!(output.contains("1 items sent longer than 5 minutes; oldest age 601s"));
        } else {
            assert!(!output.contains("oldest age"));
        }
    }
    let mut output = Vec::new();
    render_fleet_status(
        &mut output,
        &codex_router_host::FleetHostObservation::Available {
            central: Some(Box::new(central)),
            stuck_mail: collaboration_protocol::StuckMailSummary::Unavailable {
                message: "mail reader unavailable".to_owned(),
            },
        },
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("central: connected"));
    assert!(output.contains("outbox: unavailable (mail reader unavailable)"));
    assert!(!output.contains("central: unavailable"));
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn host_status_is_deterministic_complete_and_canary_free() -> std::io::Result<()> {
    let snapshot = HostSnapshot::new(HostSnapshotDimensions {
        phase: HostPhase::Steady,
        router: RouterCondition::ExternalReachable,
        app_server: AppServerCondition::NativeReady {
            running_version: "1.2.3".to_owned(),
        },
        remote_control: RemoteControlCondition::Connected,
        remote_control_identity: None,
        executable_relation: ExecutableRelation::Match,
        router_executable_relation: codex_router_host::RouterExecutableRelation::Match,
        recovery_budget: RecoveryBudget::Available,
        last_lifecycle_outcome: Some(LifecycleOutcome {
            operation: HostOperation::Start,
            classification: LifecycleOutcomeClassification::Succeeded,
        }),
    });
    let frames = [OperatorFrame::terminal(HostTerminalResponse::new(
        OperatorRequest::Status,
        TerminalClassification::Ready,
        snapshot,
        "shared Codex host status".to_owned(),
    ))];
    let mut output = Vec::new();

    render_terminal_frame(&mut output, &frames)?;

    let rendered = String::from_utf8(output).map_err(std::io::Error::other)?;
    for field in [
        "readiness:",
        "phase:",
        "router:",
        "app_server:",
        "remote_control:",
        "codex_executable_relation:",
        "router_executable_relation:",
        "recovery_budget:",
        "last_lifecycle_outcome:",
    ] {
        assert!(rendered.contains(field), "missing {field}");
    }
    assert!(!rendered.contains("PROMPT_CANARY"));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn router_drift_status_names_both_versions_and_restart_action() -> std::io::Result<()> {
    let snapshot = HostSnapshot::new(HostSnapshotDimensions {
        phase: HostPhase::Steady,
        router: RouterCondition::ExternalReachable,
        app_server: AppServerCondition::Absent,
        remote_control: RemoteControlCondition::Unavailable,
        remote_control_identity: None,
        executable_relation: ExecutableRelation::Match,
        router_executable_relation: codex_router_host::RouterExecutableRelation::Drift {
            running_version: "0.1.36".to_owned(),
            installed_version: Some("0.1.37".to_owned()),
        },
        recovery_budget: RecoveryBudget::Available,
        last_lifecycle_outcome: None,
    });
    let mut output = Vec::new();
    render_snapshot(&mut output, &snapshot)?;
    let rendered = String::from_utf8(output).map_err(std::io::Error::other)?;
    assert!(rendered.contains("router_executable_relation: drift"));
    assert!(rendered.contains("⚠ Router Host is stale (running 0.1.36, installed 0.1.37)"));
    assert!(rendered.contains("codex-router host restart"));
    let encoded = serde_json::to_value(snapshot).map_err(std::io::Error::other)?;
    assert_eq!(encoded["router_executable_relation"]["kind"], "drift");
    assert_eq!(
        encoded["router_executable_relation"]["running_version"],
        "0.1.36"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn replacement_failure_renders_the_required_manual_recovery_action() -> std::io::Result<()> {
    let mut output = Vec::new();
    render_update_result(
            &mut output,
            &UpdateResult::UpdatedButReplacementFailed {
                message: "replacement unavailable".to_owned(),
                recovery_action:
                    "start codex-router host with the same --router-root and --port after the old Host exits"
                        .to_owned(),
            },
        )?;
    let rendered = String::from_utf8(output).map_err(std::io::Error::other)?;
    assert!(rendered.contains("updated but replacement failed"));
    assert!(rendered.contains(
        "recovery_action: start codex-router host with the same --router-root and --port"
    ));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn presenter_renders_forced_shutdown_as_warning_and_failure_as_error() -> std::io::Result<()> {
    let mut presenter = HostProgressPresenter::new(false);
    let mut output = Vec::new();
    presenter.accept(
        &mut output,
        &OperatorFrame::Progress(HostProgress::AppServerKilled),
    )?;
    presenter.accept(
        &mut output,
        &OperatorFrame::Terminal(HostTerminalResponse::new(
            OperatorRequest::Status,
            TerminalClassification::Failed,
            ready_snapshot(),
            "failed".to_owned(),
        )),
    )?;
    let rendered = String::from_utf8(output).map_err(std::io::Error::other)?;
    assert!(rendered.contains("⚠ forced app-server shutdown"));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn presenter_completes_delayed_phases_in_tty_and_plain_modes() -> std::io::Result<()> {
    for tty in [false, true] {
        let mut presenter = HostProgressPresenter::new(tty);
        let mut output = Vec::new();
        presenter.accept(
            &mut output,
            &OperatorFrame::Progress(HostProgress::StartingRouter),
        )?;
        for _ in 0..20_000 {
            std::hint::spin_loop();
        }
        presenter.accept(
            &mut output,
            &OperatorFrame::Progress(HostProgress::AppServerReady),
        )?;
        let rendered = String::from_utf8(output).map_err(std::io::Error::other)?;
        assert!(rendered.contains("✓ starting router ("));
        assert!(rendered.contains("starting router ("));
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn streamed_progress_and_terminal_render_each_phase_once() -> std::io::Result<()> {
    let mut presenter = HostProgressPresenter::new(false);
    let mut output = Vec::new();
    presenter.accept(
        &mut output,
        &OperatorFrame::Progress(HostProgress::StartingAppServer),
    )?;
    presenter.accept(
        &mut output,
        &OperatorFrame::Progress(HostProgress::AppServerReady),
    )?;
    presenter.finish_success(&mut output)?;
    let response = HostTerminalResponse::new(
        OperatorRequest::RestartAppServer,
        TerminalClassification::Succeeded,
        ready_snapshot(),
        "app-server restarted".to_owned(),
    );
    render_terminal_frame(
        &mut output,
        &[
            OperatorFrame::Progress(HostProgress::StartingAppServer),
            OperatorFrame::Terminal(response),
        ],
    )?;
    let rendered = String::from_utf8(output).map_err(std::io::Error::other)?;
    assert_eq!(rendered.matches("starting app-server").count(), 1);
    assert_eq!(rendered.matches("app-server ready").count(), 1);
    assert_eq!(rendered.matches("result: succeeded").count(), 1);
    Ok(())
}

fn ready_snapshot() -> HostSnapshot {
    HostSnapshot::new(HostSnapshotDimensions {
        phase: HostPhase::Steady,
        router: RouterCondition::ExternalReachable,
        app_server: AppServerCondition::Absent,
        remote_control: RemoteControlCondition::Unavailable,
        remote_control_identity: None,
        executable_relation: ExecutableRelation::Unknown,
        router_executable_relation: codex_router_host::RouterExecutableRelation::Unknown {
            reason: "not observed".to_owned(),
        },
        recovery_budget: RecoveryBudget::Available,
        last_lifecycle_outcome: None,
    })
}

#[test]
fn status_renders_the_hosts_startup_port_values_and_sources() {
    use codex_router_host::{HostPortSelection, HostPortSource, SelectedHostPort};
    let selected = HostPortSelection {
        router: SelectedHostPort {
            port: 28741,
            source: HostPortSource::PortsFile,
        },
        mcp: SelectedHostPort {
            port: 28743,
            source: HostPortSource::Flag,
        },
        tailscale_https: SelectedHostPort {
            port: 18789,
            source: HostPortSource::Default,
        },
    };
    let snapshot = ready_snapshot().with_port_selection(Some(selected));
    let mut output = Vec::new();
    render_snapshot(&mut output, &snapshot).expect("rendered status");
    let rendered = String::from_utf8(output).expect("UTF-8 status");
    for line in [
        "router_port: 28741 (ports.json)",
        "mcp_port: 28743 (flag)",
        "tailscale_https_port: 18789 (default)",
    ] {
        assert!(rendered.contains(line), "{rendered}");
    }
    let encoded = serde_json::to_vec(&snapshot).expect("encoded snapshot");
    let decoded: HostSnapshot = serde_json::from_slice(&encoded).expect("decoded snapshot");
    assert_eq!(decoded.port_selection(), Some(selected));
}
