//! Rows29/31 use actual setup HTTP and the agreed private fixture observation.
use super::{
    TestResult, UNKNOWN,
    child_process::LauncherChild,
    observation_assertions::{assert_empty, assert_record},
    provision, push_floors_and_read,
};
use collaboration_central_conformance::signin_fixtures as signed;
use collaboration_central_core::signin_test_observation::SignInObservation;
use serde_json::{Value, json};

const OPERATION: &str = "019f0000-0000-7000-8000-00000000e601";
const NAME: &str = "Setup IPC workspace";
const DESCRIPTION: &str = "Created through the real signed setup route";

fn create_body() -> Value {
    json!({"organizationId":signed::ORGANIZATION,"operationId":OPERATION,"name":NAME,"description":DESCRIPTION})
}
fn create_intent() -> Value {
    json!({"v":1,"operation":"CreateWorkspace","organization":signed::ORGANIZATION,"target":signed::ORGANIZATION,"operationId":OPERATION,"params":{"name":NAME,"description":DESCRIPTION}})
}
fn workspace_view() -> Value {
    json!({"workspaceId":OPERATION,"name":NAME,"description":DESCRIPTION,"state":"active"})
}

async fn person_bytes(
    child: &LauncherChild,
    path: &str,
    body: &Value,
    intent: &Value,
) -> TestResult<(Vec<u8>, String)> {
    let proof_id = signed::fresh_request_id(signed::NOW_SECONDS)?;
    let owner = signed::Person::owner(signed::ORGANIZATION, signed::NOW_SECONDS)?;
    let headers = owner.headers(path, intent, &proof_id, signed::NOW_SECONDS)?;
    Ok((child.post_bytes(path, body, &headers).await?, proof_id))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Private setup observation is compared opaquely against independently named modern rows."
)]
fn assert_setup_observation(
    observed: &SignInObservation,
    statement: &str,
    used_ids: &[String],
) -> TestResult {
    let mut expected_ids = used_ids.to_vec();
    expected_ids.sort();
    let expected = json!({
        "statement":statement,
        "stamp":{"digest":signed::digest(statement),"keyId":"stamp-a","expiresAt":signed::NOW_SECONDS - 60 + signed::LIFETIME_SECONDS},
        "records":[
            {"kind":"organizationProvisioned","requestId":signed::ORGANIZATION,"targetId":signed::ORGANIZATION,"createdAtMillis":signed::NOW_SECONDS * 1000},
            {"kind":"workspaceCreated","requestId":OPERATION,"targetId":OPERATION,"createdAtMillis":signed::NOW_SECONDS * 1000}
        ],
        "organizationFloor":{"revision":3,"state":"active"},
        "personFloors":[{"subjectId":signed::OWNER_SUBJECT,"personId":signed::OWNER,"revision":4,"state":"active"}],
        "usedRequests":expected_ids.into_iter().map(|id|json!({"id":id})).collect::<Vec<_>>(),
        "workspaces":[{"workspaceId":OPERATION,"name":NAME}],
        "complete":true
    });
    assert!(
        serde_json::to_value(observed).map_err(|_| "setup observation encoding failed")?
            == expected,
        "private observation differs from the exact actual setup contract"
    );
    assert_record(
        observed,
        1,
        OPERATION,
        OPERATION,
        "workspaceCreated",
        signed::NOW_SECONDS * 1000,
    )
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A real create and lookup must preserve the final settled workspace answer shape."
)]
async fn create_lookup_and_replay(child: &LauncherChild) -> TestResult<(Vec<u8>, Vec<String>)> {
    let (created, create_id) =
        person_bytes(child, signed::CREATE, &create_body(), &create_intent()).await?;
    let parsed: Value =
        serde_json::from_slice(&created).map_err(|_| "create answer was not JSON")?;
    assert!(
        parsed == json!({"result":"applied","workspace":workspace_view()}),
        "create reply differs or retains a superseded replay flag"
    );
    let (looked_up, lookup_id) = person_bytes(
        child,
        signed::LOOKUP,
        &json!({"organizationId":signed::ORGANIZATION,"workspaceId":OPERATION}),
        &signed::lookup_intent(signed::ORGANIZATION, OPERATION),
    )
    .await?;
    let looked_up: Value =
        serde_json::from_slice(&looked_up).map_err(|_| "lookup answer was not JSON")?;
    assert!(
        looked_up == json!({"result":"found","workspace":workspace_view()}),
        "lookup did not return the actual created workspace"
    );
    let (replayed, replay_id) =
        person_bytes(child, signed::CREATE, &create_body(), &create_intent()).await?;
    assert!(
        replayed == created,
        "same operation replay did not return the first answer byte-for-byte"
    );
    Ok((created, vec![create_id, lookup_id, replay_id]))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The full public audit response independently proves Person origin omitted from minimal IPC."
)]
async fn assert_person_audit(child: &LauncherChild) -> TestResult<String> {
    let (answer, proof_id) = person_bytes(
        child,
        signed::RECORDS,
        &json!({"organizationId":signed::ORGANIZATION,"after":null,"limit":100}),
        &signed::records_intent(signed::ORGANIZATION, None, 100),
    )
    .await?;
    let answer: Value =
        serde_json::from_slice(&answer).map_err(|_| "records answer was not JSON")?;
    let records = answer
        .get("records")
        .and_then(Value::as_array)
        .ok_or("audit records absent")?;
    assert_eq!(records.len(), 2);
    let created = records.get(1).ok_or("workspace creation record absent")?;
    assert!(
        created.get("kind") == Some(&json!("workspaceCreated"))
            && created.get("requestId") == Some(&json!(OPERATION))
            && created.get("targetId") == Some(&json!(OPERATION))
            && created.get("createdAtMillis") == Some(&json!(signed::NOW_SECONDS * 1000))
            && created.get("origin")
                == Some(
                    &json!({"kind":"person","personId":signed::OWNER,"permissionChecked":"workspace:create"})
                ),
        "actual workspace audit record has the wrong Person or checked permission"
    );
    Ok(proof_id)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual child restarts preserve durable setup rows and expose an empty external fresh root."
)]
#[tokio::test]
async fn setup_private_observe_matches_real_create_lookup_replay_and_owning_reopen() -> TestResult {
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::Builder::new().prefix("sil-").tempdir()?;
    let data = root.path().join("data");
    let socket = root.path().join("s.sock");
    let mut child = LauncherChild::start(&data, &socket, &keys).await?;
    assert_empty(&child.observe(UNKNOWN).await?)?;
    assert!(
        !child.organization_file(UNKNOWN).exists(),
        "unknown private observation created storage"
    );
    let statement = signed::stamp(signed::ORGANIZATION, signed::NOW_SECONDS)?;
    provision(&child, signed::ORGANIZATION, &statement).await?;
    let mut used_ids = vec![push_floors_and_read(&child).await?];
    let (first_answer, operation_proofs) = create_lookup_and_replay(&child).await?;
    used_ids.extend(operation_proofs);
    used_ids.push(assert_person_audit(&child).await?);
    let before = child.observe(signed::ORGANIZATION).await?;
    assert_setup_observation(&before, &statement, &used_ids)?;
    child.terminate_with_stdin_open().await?;

    let mut reopened = LauncherChild::start(&data, &socket, &keys).await?;
    let after = reopened.observe(signed::ORGANIZATION).await?;
    assert!(
        after == before,
        "same owning-store restart changed the raw statement or observed setup rows"
    );
    let (replayed, proof_id) =
        person_bytes(&reopened, signed::CREATE, &create_body(), &create_intent()).await?;
    assert!(
        replayed == first_answer,
        "owning restart replay changed the first committed answer"
    );
    used_ids.push(proof_id);
    assert_setup_observation(
        &reopened.observe(signed::ORGANIZATION).await?,
        &statement,
        &used_ids,
    )?;
    reopened.terminate_with_stdin_open().await?;

    let fresh_root = tempfile::Builder::new().prefix("sil-").tempdir()?;
    let mut fresh = LauncherChild::start(
        &fresh_root.path().join("data"),
        &fresh_root.path().join("s.sock"),
        &keys,
    )
    .await?;
    assert_empty(&fresh.observe(signed::ORGANIZATION).await?)?;
    assert!(
        !fresh.organization_file(signed::ORGANIZATION).exists(),
        "external fresh restart inherited prior setup storage"
    );
    fresh.interrupt_with_stdin_open().await?;
    keys.shutdown().await
}
