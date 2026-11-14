//! The launcher consumer's real child/Unix-link/stdin/stderr lifecycle.
mod child_process;
mod clock_boundaries;
mod observation_assertions;
mod setup_ipc;

use child_process::LauncherChild;
use collaboration_central_conformance::signin_fixtures as signed;
use observation_assertions::{
    assert_empty, assert_floors_and_used_request, assert_record, assert_stamp,
};
use serde_json::json;

type TestResult<TValue = ()> = collaboration_central_conformance::ConformanceResult<TValue>;
const PROVISION: &str = "/internal/organization-provisioning";
const REVISIONS: &str = "/internal/organization-authorization-revisions";
const SECOND: &str = "019f0000-0000-7000-8000-00000000e5b1";
const UNKNOWN: &str = "019f0000-0000-7000-8000-00000000e5c1";

async fn provision(child: &LauncherChild, organization: &str, statement: &str) -> TestResult {
    let answer = child
        .post(
            PROVISION,
            &json!({"organizationId":organization,"organizationStatement":statement}),
            &[],
        )
        .await?;
    if answer != json!({"result":"provisioned"}) {
        return Err("real private provisioning returned another closed answer".into());
    }
    Ok(())
}

async fn push_floors_and_read(child: &LauncherChild) -> TestResult<String> {
    let organization = json!({"kind":"organization","organizationId":signed::ORGANIZATION,"revision":3,"state":"active","permissionPolicyDigest":signed::POLICY});
    let person = json!({"kind":"person","organizationId":signed::ORGANIZATION,"personId":signed::OWNER,"subjectId":signed::OWNER_SUBJECT,"revision":4,"state":"active"});
    for (body, revision) in [(organization, 3), (person, 4)] {
        let answer = child.post(REVISIONS, &body, &[]).await?;
        if answer != json!({"acknowledgedRevision":revision}) {
            return Err("real private revision did not acknowledge the retained floor".into());
        }
    }
    let request = signed::fresh_request_id(signed::NOW_SECONDS)?;
    let owner = signed::Person::owner(signed::ORGANIZATION, signed::NOW_SECONDS)?;
    let headers = owner.headers(
        signed::RECORDS,
        &signed::records_intent(signed::ORGANIZATION, None, 100),
        &request,
        signed::NOW_SECONDS,
    )?;
    let answer = child
        .post(
            signed::RECORDS,
            &json!({"organizationId":signed::ORGANIZATION,"after":null,"limit":100}),
            &headers,
        )
        .await?;
    if answer.get("result") != Some(&json!("records")) {
        return Err("real signed records read was not admitted".into());
    }
    Ok(request)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Independent child IPC, actual HTTP state and signal lifecycle assertions must fail without dumping signed material."
)]
#[tokio::test]
async fn real_launcher_keeps_private_ipc_clock_observation_and_fresh_restart_contracts()
-> TestResult {
    // Wrong builds caught: parser-only readiness, wrong socket permissions,
    // unknown observation opening a store, one clock not reaching live/future
    // orgs, fabricated observation rows, command refusal ending the process,
    // either shutdown signal ignored, or shutdown waiting for stdin EOF.
    let keys = signed::SignInKeys::start().await?;
    let first_root = tempfile::Builder::new().prefix("sil-").tempdir()?;
    let mut child = LauncherChild::start(
        &first_root.path().join("data"),
        &first_root.path().join("s.sock"),
        &keys,
    )
    .await?;
    assert!(
        !child.legacy_file_exists(),
        "accepted legacy flag created a legacy store"
    );
    assert!(!child.organization_file(UNKNOWN).exists());
    assert_empty(&child.observe(UNKNOWN).await?)?;
    assert!(
        !child.organization_file(UNKNOWN).exists(),
        "observing an unknown org created its store"
    );
    assert_eq!(child.set_time_ms(signed::NOW_SECONDS * 1000).await?, 0);

    let first = signed::stamp(signed::ORGANIZATION, signed::NOW_SECONDS)?;
    provision(&child, signed::ORGANIZATION, &first).await?;
    assert!(
        child.organization_file(signed::ORGANIZATION).is_file(),
        "positive control did not create the expected owning store"
    );
    let initial = child.observe(signed::ORGANIZATION).await?;
    assert_stamp(
        &initial,
        &first,
        signed::NOW_SECONDS - 60 + signed::LIFETIME_SECONDS,
    )?;
    assert_eq!(initial.records.len(), 1);
    assert_record(
        &initial,
        0,
        signed::ORGANIZATION,
        signed::ORGANIZATION,
        "organizationProvisioned",
        signed::NOW_SECONDS * 1000,
    )?;
    assert!(
        initial.organization_floor.is_none()
            && initial.person_floors.is_empty()
            && initial.used_requests.is_empty()
    );
    let request = push_floors_and_read(&child).await?;
    let admitted = child.observe(signed::ORGANIZATION).await?;
    assert_stamp(
        &admitted,
        &first,
        signed::NOW_SECONDS - 60 + signed::LIFETIME_SECONDS,
    )?;
    assert_floors_and_used_request(&admitted, &request)?;

    let moved = signed::NOW_SECONDS + 3_600;
    assert_eq!(child.set_time_ms(moved * 1000).await?, 1);
    let renewal = signed::renewal(signed::ORGANIZATION, &first, moved, 0)?;
    provision(&child, signed::ORGANIZATION, &renewal).await?;
    let renewed = child.observe(signed::ORGANIZATION).await?;
    assert_stamp(&renewed, &renewal, moved - 60 + signed::LIFETIME_SECONDS)?;
    assert_eq!(renewed.records.len(), 2);
    assert_record(
        &renewed,
        1,
        signed::ORGANIZATION,
        &signed::digest(&renewal),
        "stampRenewed",
        moved * 1000,
    )?;
    assert!(
        renewed.records.first() == initial.records.first(),
        "renewal rewrote the committed setup record"
    );

    assert!(!child.organization_file(SECOND).exists());
    let later = signed::stamp(SECOND, moved)?;
    provision(&child, SECOND, &later).await?;
    let later_observation = child.observe(SECOND).await?;
    assert_stamp(
        &later_observation,
        &later,
        moved - 60 + signed::LIFETIME_SECONDS,
    )?;
    assert_eq!(later_observation.records.len(), 1);
    assert_record(
        &later_observation,
        0,
        SECOND,
        SECOND,
        "organizationProvisioned",
        moved * 1000,
    )?;
    assert_eq!(child.set_time_ms(moved * 1000).await?, 2);
    for invalid_command in [json!({"restartFresh":true}), json!({"setTimeMs":1.5})] {
        child.expect_refused(&invalid_command).await?;
        assert_eq!(
            child.set_time_ms(moved * 1000).await?,
            2,
            "the next valid command must still answer after each refusal"
        );
        assert!(
            child.observe(signed::ORGANIZATION).await? == renewed,
            "a refused command must preserve the existing organization observation"
        );
    }
    child.terminate_with_stdin_open().await?;

    // The only fresh restart is an external new process on a fresh root.
    // The restartFresh request above was refused; no live-registry reset occurred.
    let fresh_root = tempfile::Builder::new().prefix("sil-").tempdir()?;
    let mut fresh = LauncherChild::start(
        &fresh_root.path().join("data"),
        &fresh_root.path().join("s.sock"),
        &keys,
    )
    .await?;
    assert_eq!(fresh.set_time_ms(moved * 1000).await?, 0);
    assert_empty(&fresh.observe(signed::ORGANIZATION).await?)?;
    assert_empty(&fresh.observe(SECOND).await?)?;
    assert!(
        !fresh.organization_file(signed::ORGANIZATION).exists()
            && !fresh.organization_file(SECOND).exists(),
        "fresh-store observation recreated a previous org"
    );
    fresh.interrupt_with_stdin_open().await?;
    keys.shutdown().await
}
