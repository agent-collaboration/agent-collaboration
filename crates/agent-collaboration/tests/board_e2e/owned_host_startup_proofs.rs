//! Protected-port observations exercise the real owned Host restart boundary.
use super::owned_host::{BoardHost, protected_listener_port};
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use std::{os::unix::fs::DirBuilderExt as _, path::PathBuf};

fn private_host_directory() -> FixtureResult<(tempfile::TempDir, PathBuf)> {
    let parent = std::env::var_os("CODEX_ROUTER_E2E_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = tempfile::Builder::new()
        .prefix(&format!(
            "{}-host-protected-port-",
            chrono::Utc::now().format("%Y-%m")
        ))
        .tempdir_in(parent)?;
    let directory = root.path().join("host");
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    Ok((root, directory))
}

async fn stop_owned_host(owned: Option<BoardHost>) -> FixtureResult {
    match owned {
        Some(host) => host.stop().await,
        None => Ok(()),
    }
}

#[test]
fn a_port_report_cannot_permit_any_protected_actual_listener() {
    for port in [8787, 8788, 19741, 19742, 19743, 43127] {
        assert_eq!(protected_listener_port(port, 45000), Some(port));
        assert_eq!(protected_listener_port(45000, port), Some(port));
    }
    assert_eq!(protected_listener_port(45000, 45001), None);
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
async fn a_protected_ready_port_is_joined_before_a_new_host_is_accepted() -> FixtureResult {
    let (_root, directory) = private_host_directory()?;
    let hold_marker = directory.join("fixture-hold-before-serve");
    std::fs::write(&hold_marker, [])?;
    run_scenario(
        None::<BoardHost>,
        async |owned| {
            let mut reports = 0;
            let mut removed_hold = Ok(());
            let (host, stopped) = BoardHost::start_with_port_reports(&directory, None, |port| {
                reports += 1;
                if reports == 1 {
                    // read_ready has acknowledged the child is held before serve entry.
                    // Its captured hold survives removal; only later children serve normally.
                    removed_hold = std::fs::remove_file(&hold_marker);
                    43127
                } else {
                    port
                }
            })
            .await?;
            *owned = Some(host);
            removed_hold?;
            assert!(!stopped.is_empty(), "forced protected report was accepted");
            assert_eq!(reports, stopped.len() + 1);
            for rejected in stopped {
                // The fixture issues this token only after stop has reaped and drained.
                assert_eq!(rejected.directory(), directory.canonicalize()?);
            }
            let accepted = owned.as_ref().ok_or("accepted Host is absent")?;
            let port = reqwest::Url::parse(&accepted.mcp_url)?.port();
            assert!(!matches!(
                port,
                None | Some(8787 | 8788 | 19741 | 19742 | 19743 | 43127)
            ));
            Ok(())
        },
        stop_owned_host,
        || Ok(()),
    )
    .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
async fn repeated_protected_ready_ports_stop_after_three_joined_attempts() -> FixtureResult {
    let (_root, directory) = private_host_directory()?;
    run_scenario(
        None::<BoardHost>,
        async |owned| {
            let mut reports = 0;
            let result = BoardHost::start_with_port_reports(&directory, None, |_| {
                reports += 1;
                43127
            })
            .await;
            let error = match result {
                Ok((host, _)) => {
                    *owned = Some(host);
                    return Err("protected ports exhausted but a Host was accepted".into());
                }
                Err(error) => error,
            };
            assert_eq!(reports, 3, "protected-port restarts were not bounded");
            assert!(
                error
                    .to_string()
                    .contains("protected listener ports [43127, 43127, 43127] after 3 attempts")
            );
            // Exhaustion must release the same directory for another real Host.
            *owned = Some(BoardHost::start(&directory, None).await?);
            Ok(())
        },
        stop_owned_host,
        || Ok(()),
    )
    .await
}
