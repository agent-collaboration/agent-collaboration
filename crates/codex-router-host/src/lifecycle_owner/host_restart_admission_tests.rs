//! Invalid replacement inputs refuse at admission without scheduling teardown.

use super::request_admission::*;
use super::*;
use crate::OperatorFrame;
use std::net::TcpListener;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdmissionCase {
    Malformed,
    MalformedRouterOnly,
    OccupiedRouter,
    OccupiedMcp,
    OccupiedHttps,
    InactiveHttps,
    KeptPorts,
    FreeMcp,
    ExplicitMcp,
}

#[tokio::test]
async fn malformed_restart_ports_preserve_the_admitted_host_state() {
    for scenario in [
        AdmissionCase::Malformed,
        AdmissionCase::MalformedRouterOnly,
        AdmissionCase::OccupiedRouter,
        AdmissionCase::OccupiedHttps,
        AdmissionCase::InactiveHttps,
        AdmissionCase::KeptPorts,
    ] {
        assert_restart_admission(scenario).await;
    }
}

#[tokio::test]
async fn occupied_restart_mcp_port_preserves_the_admitted_host_state() {
    for scenario in [
        AdmissionCase::OccupiedMcp,
        AdmissionCase::FreeMcp,
        AdmissionCase::ExplicitMcp,
    ] {
        assert_restart_admission(scenario).await;
    }
}

async fn assert_restart_admission(scenario: AdmissionCase) {
    let refuses = matches!(
        scenario,
        AdmissionCase::Malformed
            | AdmissionCase::MalformedRouterOnly
            | AdmissionCase::OccupiedRouter
            | AdmissionCase::OccupiedMcp
            | AdmissionCase::OccupiedHttps
            | AdmissionCase::InactiveHttps
    );
    let root = tempfile::tempdir().expect("root");
    let router_listener = TcpListener::bind("127.0.0.1:0").expect("held router");
    let mcp_listener = TcpListener::bind("127.0.0.1:0").expect("held MCP");
    let competing_listener = TcpListener::bind("127.0.0.1:0").expect("competing MCP");
    let router = router_listener.local_addr().expect("router address");
    let mcp = mcp_listener.local_addr().expect("MCP address");
    let competing = competing_listener.local_addr().expect("competing address");
    let https_listener = TcpListener::bind("127.0.0.1:0").expect("HTTPS port");
    let https = https_listener.local_addr().expect("HTTPS address");
    let exposes_https = matches!(
        scenario,
        AdmissionCase::KeptPorts | AdmissionCase::OccupiedHttps
    );
    let _held_https = exposes_https.then_some(https_listener);
    if scenario == AdmissionCase::FreeMcp {
        drop(competing_listener);
    }
    let path = root.path().join("ports.json");
    let file = if matches!(
        scenario,
        AdmissionCase::Malformed | AdmissionCase::MalformedRouterOnly
    ) {
        "{".to_owned()
    } else {
        let next_router = if scenario == AdmissionCase::OccupiedRouter {
            competing.port()
        } else {
            router.port()
        };
        let next_mcp = if matches!(
            scenario,
            AdmissionCase::OccupiedMcp | AdmissionCase::FreeMcp | AdmissionCase::ExplicitMcp
        ) {
            competing.port()
        } else {
            mcp.port()
        };
        let next_https = if matches!(
            scenario,
            AdmissionCase::OccupiedHttps | AdmissionCase::InactiveHttps
        ) {
            competing.port()
        } else {
            https.port()
        };
        format!("{{\"router\":{next_router},\"mcp\":{next_mcp},\"tailscaleHttps\":{next_https}}}")
    };
    std::fs::write(&path, file).expect("file");
    let selection = crate::HostPortSelection {
        router: crate::SelectedHostPort {
            port: router.port(),
            source: crate::HostPortSource::PortsFile,
        },
        mcp: crate::SelectedHostPort {
            port: mcp.port(),
            source: if scenario == AdmissionCase::ExplicitMcp {
                crate::HostPortSource::Flag
            } else {
                crate::HostPortSource::PortsFile
            },
        },
        tailscale_https: crate::SelectedHostPort {
            port: https.port(),
            source: crate::HostPortSource::PortsFile,
        },
    };
    let executable = PathBuf::from("/bin/sh");
    let mut config = HostConfig::new(crate::HostConfigInputs {
        coordination_paths: crate::HostCoordinationPaths::for_router_root(root.path()),
        router_endpoint: router,
        mcp_bind: mcp,
        app_server_socket: root.path().join("app.sock"),
        managed_executable: executable.clone(),
        deadlines: crate::HostDeadlines::production(),
    })
    .with_port_selection(selection);
    if exposes_https {
        config = config.with_remote_access(crate::RemoteAccess::tailscale(
            std::num::NonZeroU16::new(https.port()).expect("HTTPS port"),
        ));
    }
    let identity = codex_native_integration::executable_identity(&executable)
        .await
        .expect("identity");
    let plans = ManagedChildLaunchPlans::new(
        Some(ChildCommandSpec::new(executable.clone())),
        crate::AppServerLaunchPlan::new(
            ChildCommandSpec::new(executable.clone()),
            identity,
            "test".to_owned(),
        ),
    );
    let updates = ManagedUpdateInputs::production()
        .with_replacement_command(ChildCommandSpec::new(executable.clone()));
    let mut state = RuntimeState::ready(
        if scenario == AdmissionCase::MalformedRouterOnly {
            RouterCondition::OwnedReachable
        } else {
            RouterCondition::ExternalReachable
        },
        crate::AppServerReadiness::Ready {
            running_version: "test".to_owned(),
            remote_control_server_name: None,
        },
    );
    let before = state.snapshot();
    let mut app_server = None;
    let mut router_child = None;
    let mut active_app_server_restart = None;
    let mut active_router_restart = None;
    let mut active_update = None;
    let mut active_host_replacement = None;
    let mut active_status = None;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(16);
    let (_ack, reexecuting_ack) = tokio::sync::oneshot::channel();
    handle_operator_work(
        OperatorWork {
            request: if scenario == AdmissionCase::MalformedRouterOnly {
                OperatorRequest::RestartRouter
            } else {
                OperatorRequest::RestartHost {
                    executable: executable.clone(),
                }
            },
            response: sender,
            reexecuting_ack,
        },
        OperatorRuntimeContext {
            state: &mut state,
            app_server: &mut app_server,
            router_child: &mut router_child,
            config: &config,
            mcp_listen_address: Some(mcp),
            child_launch_plans: &plans,
            update_inputs: &updates,
            active_app_server_restart: &mut active_app_server_restart,
            active_router_restart: &mut active_router_restart,
            active_update: &mut active_update,
            active_host_replacement: &mut active_host_replacement,
            active_status: &mut active_status,
            update_drain_active: false,
            router_executable_observer: std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::RouterExecutableObserver::capture(Ok(executable), "test".to_owned()),
            )),
        },
    )
    .await;
    let frame = receiver.try_recv().expect("admission response");
    if !refuses {
        assert!(
            matches!(
                frame,
                OperatorFrame::Progress(crate::HostProgress::ReplacementStarting)
            ),
            "{scenario:?}: {frame:?}"
        );
        assert!(
            active_host_replacement.is_some(),
            "{scenario:?}: replacement must be admitted"
        );
        return;
    }
    let OperatorFrame::Terminal(response) = frame else {
        panic!("invalid replacement began teardown: {frame:?}")
    };
    assert_eq!(response.classification(), TerminalClassification::Failed);
    assert!(
        response.message().contains(path.to_str().expect("path")),
        "{}",
        response.message()
    );
    if !matches!(
        scenario,
        AdmissionCase::Malformed | AdmissionCase::MalformedRouterOnly
    ) {
        assert!(
            response.message().contains(&competing.port().to_string()),
            "{}",
            response.message()
        );
    }
    assert!(
        active_host_replacement.is_none(),
        "teardown must not be scheduled"
    );
    assert!(
        active_router_restart.is_none(),
        "Router teardown must not be scheduled"
    );
    assert_eq!(
        state.snapshot(),
        before,
        "ready state must remain unchanged"
    );
}
