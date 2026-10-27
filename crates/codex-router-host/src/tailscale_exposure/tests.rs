use super::*;
use crate::ChildCommandSpec;
use std::{num::NonZeroU16, path::PathBuf};

const PREFLIGHT_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/tailscale-preflight-agent.ts"
);

fn fixture(mode: &str) -> TailscaleCommand {
    TailscaleCommand::fixture(
        ChildCommandSpec::new(PathBuf::from("node"))
            .with_arguments([PREFLIGHT_FIXTURE])
            .with_environment("TAILNET_TEST_MODE", mode),
    )
}

fn access() -> RemoteAccess {
    RemoteAccess::tailscale(NonZeroU16::new(8443).expect("port"))
}

#[tokio::test]
async fn disabled_mode_performs_no_tailscale_io_even_with_an_unusable_executable() {
    let command = TailscaleCommand::fixture(ChildCommandSpec::new(PathBuf::from(
        "/missing/tailscale-must-not-run",
    )));
    assert!(
        PreparedTailscaleExposure::prepare_with_command(None, command)
            .await
            .expect("disabled mode")
            .is_none()
    );
}

#[tokio::test]
async fn preflight_rejects_foreign_cli_failures_before_granting_proxy_authority() {
    for mode in ["malformed", "offline", "no_https", "collision"] {
        let result =
            PreparedTailscaleExposure::prepare_with_command(Some(access()), fixture(mode)).await;
        assert!(
            result.is_err(),
            "{mode} unexpectedly granted a proxy authority"
        );
    }
    let prepared =
        PreparedTailscaleExposure::prepare_with_command(Some(access()), fixture("normal"))
            .await
            .expect("preflight")
            .expect("opted in");
    assert_eq!(prepared.authority().authority(), "node.example.ts.net:8443");
}

const LEASE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/tailscale-lease-agent.ts"
);

#[tokio::test]
async fn shutdown_reaps_the_owned_child_and_verifies_only_its_foreground_lease_retired() {
    for keep_lease in [false, true] {
        let directory = tempfile::tempdir().expect("private state directory");
        let state_path = directory.path().join("foreign-serve-state.json");
        let command = TailscaleCommand::fixture(
            ChildCommandSpec::new(PathBuf::from("node"))
                .with_arguments([LEASE_FIXTURE])
                .with_environment("TAILNET_TEST_STATE", &state_path)
                .with_environment(
                    "TAILNET_TEST_KEEP_LEASE",
                    if keep_lease { "yes" } else { "no" },
                ),
        );
        let serve_child = TailscaleCommand::fixture(
            ChildCommandSpec::new(std::env::current_exe().expect("Host unit-test binary"))
                .with_arguments([
                    "--exact",
                    "tailscale_exposure::tests::isolated_foreground_lease_fixture",
                    "--ignored",
                    "--nocapture",
                    "--",
                ])
                .with_environment("TAILNET_TEST_STATE", &state_path)
                .with_environment(
                    "TAILNET_TEST_KEEP_LEASE",
                    if keep_lease { "yes" } else { "no" },
                ),
        );
        let mut process = serve_child
            .spawn_serve(
                NonZeroU16::new(8443).expect("port"),
                "127.0.0.1:19088".parse().expect("backend"),
                "node.example.ts.net",
            )
            .expect("real child");
        let pid = process.process_id().expect("pid");
        process
            .advertised()
            .await
            .expect("foreign child advertises");
        // This test begins after advertisement; it makes no TLS/readiness claim.
        let mut exposure = TailscaleExposure {
            command,
            process,
            session_id: ServeSessionId::try_from("fixture-owned".to_owned()).expect("session ID"),
            url: "https://node.example.ts.net:8443/mcp".to_owned(),
        };
        let result = exposure.shutdown().await;
        if keep_lease {
            let error = result.expect_err("cleanup must refuse a retained own lease");
            assert_eq!(
                error.to_string(),
                "Tailscale foreground Serve session remains after its child was reaped"
            );
        } else {
            result.expect("normal foreground lease shutdown must succeed");
        }
        let remaining: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&state_path).expect("state readback"))
                .expect("state");
        assert_eq!(
            remaining["Foreground"]["foreign"],
            serde_json::json!({"TCP":{"443":{"HTTPS":true}}}),
            "unrelated foreground lease changed"
        );
        let pid = rustix::process::Pid::from_raw(i32::try_from(pid).expect("pid fits"))
            .expect("positive pid");
        assert!(
            rustix::process::test_kill_process(pid).is_err(),
            "owned Serve child was not reaped"
        );
    }
}

#[tokio::test]
#[ignore = "Private foreground Serve fixture child, not a behavior test"]
async fn isolated_foreground_lease_fixture() {
    use std::io::Write as _;

    // Register before publishing readiness: an interrupt arriving before recv
    // is polled remains latched, unlike the old Python pause fixture.
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .expect("fixture SIGINT listener");
    let state_path =
        PathBuf::from(std::env::var_os("TAILNET_TEST_STATE").expect("private state path"));
    let keep_lease = std::env::var("TAILNET_TEST_KEEP_LEASE").expect("lease mode") == "yes";
    let mut lease_state = serde_json::json!({
        "Foreground": {
            "foreign": {"TCP": {"443": {"HTTPS": true}}},
            "fixture-owned": {"TCP": {"8443": {"HTTPS": true}}}
        }
    });
    std::fs::write(
        &state_path,
        serde_json::to_vec(&lease_state).expect("lease JSON"),
    )
    .expect("initial lease state");
    {
        let mut output = std::io::stdout().lock();
        writeln!(output, "https://node.example.ts.net:8443/").expect("fixture advertisement");
        output.flush().expect("flushed fixture advertisement");
    }
    interrupt.recv().await.expect("fixture interrupt");
    if !keep_lease {
        lease_state["Foreground"]
            .as_object_mut()
            .expect("foreground leases")
            .remove("fixture-owned")
            .expect("owned fixture lease");
        std::fs::write(
            &state_path,
            serde_json::to_vec(&lease_state).expect("lease JSON"),
        )
        .expect("retired owned lease state");
    }
}
