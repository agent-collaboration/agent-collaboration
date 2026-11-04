use super::cutover_host_fixture::CutoverHostFixture;
use codex_router_host::{HostCoordinationPaths, HostInstanceLock, InstanceAcquireError};
use serde_json::json;
use std::{
    path::Path,
    process::{Command, Output},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the public pre-stop inspection outcome."
)]
fn compiled_inspection_preserves_the_running_host_and_reports_its_declared_identity() -> TestResult
{
    // Arrange an existing owned Host identity and the same real singleton lock.
    let fixture = CutoverHostFixture::new(None)?;
    let paths = HostCoordinationPaths::for_router_root(&fixture.root);
    let host = HostInstanceLock::acquire(&paths)?;
    let before = std::fs::read(fixture.identity_path("installation-peer.json"))?;
    // Act through the normal compiled helper, signed at its direct launch boundary.
    let output = inspect(&fixture, String::from(fixture.service.clone()))?;
    // Assert actual process success and exact metadata without taking live authority.
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout)?,
        json!({
            "kind":"hostInspected", "routerRoot":std::fs::canonicalize(&fixture.root)?,
            "serviceId":fixture.service, "peerId":fixture.peer,
        })
    );
    assert_eq!(
        std::fs::read(fixture.identity_path("installation-peer.json"))?,
        before
    );
    assert!(matches!(
        HostInstanceLock::acquire(&paths),
        Err(InstanceAcquireError::AlreadyRunning)
    ));
    drop(host);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert refusal before any Host mutation."
)]
fn compiled_inspection_refuses_another_declared_service_without_replacing_identity() -> TestResult {
    // Arrange a live Host whose identity differs from the operator inventory.
    let fixture = CutoverHostFixture::new(None)?;
    let paths = HostCoordinationPaths::for_router_root(&fixture.root);
    let host = HostInstanceLock::acquire(&paths)?;
    let before = std::fs::read(fixture.identity_path("service-identity.json"))?;
    // Act with a valid UUIDv7 belonging to another service.
    let output = inspect(&fixture, uuid::Uuid::now_v7().to_string())?;
    // Assert the real CLI refuses, emits no success metadata, and preserves authority.
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("service differs"));
    assert_eq!(
        std::fs::read(fixture.identity_path("service-identity.json"))?,
        before
    );
    assert!(matches!(
        HostInstanceLock::acquire(&paths),
        Err(InstanceAcquireError::AlreadyRunning)
    ));
    drop(host);
    Ok(())
}

fn inspect(fixture: &CutoverHostFixture, service: String) -> std::io::Result<Output> {
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    Command::new(runner)
        .arg(env!("CARGO_BIN_EXE_codex-router-board-staging-cutover"))
        .arg("inspect")
        .arg("--router-root")
        .arg(&fixture.root)
        .arg("--service-id")
        .arg(service)
        .arg("--peer-id")
        .arg(fixture.peer.to_string())
        .output()
}
