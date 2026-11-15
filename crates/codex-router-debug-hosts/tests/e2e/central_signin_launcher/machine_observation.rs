//! Actual launcher machine decisions followed by its private observation IPC.
use super::{TestResult, child_process::LauncherChild, provision, push_floors_and_read};
use collaboration_central_conformance::signin_fixtures as signed;
use collaboration_central_core::signin_test_observation::SignInObservation;
use serde_json::{Value, json};

const DEVICE: &str = "/internal/device-login-decisions";
const TOKEN: &str = "/internal/machine-token-decisions";
const INSTALLATION: &str = "019f0000-0000-7000-8000-00000000f001";
const OTHER_INSTALLATION: &str = "019f0000-0000-7000-8000-00000000f002";
const FIRST_WORKSPACE: &str = "019f0000-0000-7000-8000-00000000f101";
const SECOND_WORKSPACE: &str = "019f0000-0000-7000-8000-00000000f102";
const FIRST_REQUEST: &str = "019f0000-0000-7000-8000-00000000f201";
const SECOND_REQUEST: &str = "019f0000-0000-7000-8000-00000000f202";
const LAPSED_REQUEST: &str = "019f0000-0000-7000-8000-00000000f203";
const OTHER_MACHINE_REQUEST: &str = "019f0000-0000-7000-8000-00000000f204";

fn approval(request: &str, label: &str, workspaces: &[&str], now: i64) -> Value {
    json!({"organizationId":signed::ORGANIZATION,"approver":signed::OWNER,
        "permissionChecked":"machine:enroll","requestId":request,
        "installation":INSTALLATION,"peer":INSTALLATION,"label":label,
        "allowedWorkspaces":workspaces,"approverSecondFactorAt":now-60,
        "codeExpiresAt":now+600})
}

async fn approve(child: &LauncherChild, body: &Value) -> TestResult<String> {
    let answer = child.post(DEVICE, body, &[]).await?;
    let enrollment = answer
        .get("enrollmentId")
        .and_then(Value::as_str)
        .ok_or("approval did not return an enrollment identity")?;
    message_board::RecordId::try_from(enrollment.to_owned())
        .map_err(|_| "approval did not return a canonical UUIDv7")?;
    if answer
        != json!({"result":"approved","enrollmentId":enrollment,
        "label":body["label"],"allowedWorkspaces":body["allowedWorkspaces"]})
    {
        return Err("approval reply differs from its original snapshot".into());
    }
    Ok(enrollment.to_owned())
}

async fn token(child: &LauncherChild, enrollment: &str) -> TestResult<Value> {
    child
        .post(
            TOKEN,
            &json!({"organizationId":signed::ORGANIZATION,
        "enrollmentId":enrollment,"clientId":enrollment}),
            &[],
        )
        .await
}

fn grant(enrollment: &str, now: i64, predecessor: Option<&str>) -> Value {
    let mut answer = json!({"result":"grant","installation":INSTALLATION,
        "enrollmentId":enrollment,"decidedAt":now,"grantExpiresAt":now+600});
    if let Some(predecessor) = predecessor {
        answer["superseded"] = json!({"enrollmentId":predecessor,"clientId":predecessor});
    }
    answer
}

fn utc(seconds: i64) -> TestResult<String> {
    Ok(chrono::DateTime::from_timestamp(seconds, 0)
        .ok_or("fixture timestamp is out of range")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The IPC contract must expose complete machine arrays and no credential surface without dumping its signed statement."
)]
fn machine_arrays(observation: &SignInObservation) -> TestResult<(Value, Value)> {
    let encoded =
        serde_json::to_value(observation).map_err(|_| "machine observation encoding failed")?;
    let object = encoded.as_object().ok_or("observation is not an object")?;
    let mut fields = object.keys().map(String::as_str).collect::<Vec<_>>();
    fields.sort_unstable();
    assert!(
        fields
            == [
                "complete",
                "machineApprovals",
                "machines",
                "organizationFloor",
                "personFloors",
                "records",
                "stamp",
                "statement",
                "usedRequests",
                "workspaces"
            ],
        "observation top-level fields differ or expose credentials"
    );
    assert!(observation.complete, "machine observation is incomplete");
    let machines = encoded
        .get("machines")
        .filter(|rows| rows.is_array())
        .ok_or("machine observation lacks machines array")?
        .clone();
    let approvals = encoded
        .get("machineApprovals")
        .filter(|rows| rows.is_array())
        .ok_or("machine observation lacks machineApprovals array")?
        .clone();
    Ok((machines, approvals))
}

async fn create_workspaces(child: &LauncherChild) -> TestResult {
    push_floors_and_read(child).await?;
    for (operation, name) in [
        (FIRST_WORKSPACE, "Machine observation first"),
        (SECOND_WORKSPACE, "Machine observation second"),
    ] {
        let body = json!({"organizationId":signed::ORGANIZATION,"operationId":operation,
            "name":name,"description":"Created over the launcher's signed private route"});
        let intent = json!({"v":1,"operation":"CreateWorkspace",
            "organization":signed::ORGANIZATION,"target":signed::ORGANIZATION,
            "operationId":operation,"params":{"name":name,
            "description":"Created over the launcher's signed private route"}});
        let owner = signed::Person::owner(signed::ORGANIZATION, signed::NOW_SECONDS)?;
        let headers = owner.headers(
            signed::CREATE,
            &intent,
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
            signed::NOW_SECONDS,
        )?;
        let answer = child.post(signed::CREATE, &body, &headers).await?;
        if answer
            != json!({"result":"applied","workspace":{"workspaceId":operation,
            "name":name,"description":"Created over the launcher's signed private route",
            "state":"active"}})
        {
            return Err("machine observation workspace precondition was not created".into());
        }
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real private HTTP effects must match every agreed machine observation field without exposing raw statement values."
)]
#[tokio::test]
async fn private_observe_projects_machine_and_approval_fields_with_independent_allowances_and_created_order()
-> TestResult {
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::Builder::new().prefix("sim-").tempdir()?;
    let mut child = LauncherChild::start(
        &root.path().join("data"),
        &root.path().join("s.sock"),
        &keys,
    )
    .await?;
    let now = signed::NOW_SECONDS;
    provision(
        &child,
        signed::ORGANIZATION,
        &signed::stamp(signed::ORGANIZATION, now)?,
    )
    .await?;
    create_workspaces(&child).await?;

    // Approved admits nothing; all three absent lifecycle times/identities
    // remain JSON null, while the first immutable allowance snapshot is visible.
    let a = approve(
        &child,
        &approval(FIRST_REQUEST, "Observed first", &[FIRST_WORKSPACE], now),
    )
    .await?;
    let (machines, approvals) = machine_arrays(&child.observe(signed::ORGANIZATION).await?)?;
    let machine_id = machines
        .pointer("/0/machineId")
        .and_then(Value::as_str)
        .ok_or("observed machine identity is missing")?
        .to_owned();
    message_board::RecordId::try_from(machine_id.clone())
        .map_err(|_| "observed machine identity is not a canonical UUIDv7")?;
    assert!(
        machines
            == json!([{"machineId":machine_id,"installationId":INSTALLATION,
        "state":"revoked","allowedWorkspaces":[FIRST_WORKSPACE],"enrollmentGeneration":0}]),
        "approved-only machine observation differs"
    );
    assert!(
        approvals
            == json!([{"approvalId":a,"requestId":FIRST_REQUEST,"machineId":machine_id,
        "state":"approved","endReason":null,"allowedWorkspaces":[FIRST_WORKSPACE],
        "baseGeneration":0,"codeExpiresAt":utc(now+600)?,"machineClientId":null,
        "lastGrantAt":null,"endedAt":null,"createdAt":utc(now)?}]),
        "approved snapshot fields, canonical times or nulls differ"
    );

    assert_eq!(child.set_time_ms((now + 1) * 1000).await?, 1);
    assert!(
        token(&child, &a).await? == grant(&a, now + 1, None),
        "first grant differs"
    );
    assert_eq!(child.set_time_ms((now + 2) * 1000).await?, 1);
    let b = approve(
        &child,
        &approval(
            SECOND_REQUEST,
            "Observed second",
            &[SECOND_WORKSPACE],
            now + 2,
        ),
    )
    .await?;
    assert_eq!(child.set_time_ms((now + 3) * 1000).await?, 1);
    assert!(
        token(&child, &b).await? == grant(&b, now + 3, Some(&a)),
        "superseding grant differs"
    );

    // The existing backward-clock command makes createdAt order differ from
    // insertion order. No store mutation or additional fixture seam is needed.
    assert_eq!(child.set_time_ms((now - 1) * 1000).await?, 1);
    let mut pending = approval(LAPSED_REQUEST, "Observed lapse", &[], now - 1);
    pending["codeExpiresAt"] = json!(now + 4);
    let c = approve(&child, &pending).await?;
    assert_eq!(child.set_time_ms((now + 4) * 1000).await?, 1);
    assert!(
        token(&child, &c).await?
            == json!({"result":"refuse","reason":{"kind":"enrollmentNotActive"}}),
        "expired pending code was not refused"
    );
    let (machines, approvals) = machine_arrays(&child.observe(signed::ORGANIZATION).await?)?;
    assert!(
        machines
            == json!([{"machineId":machine_id,"installationId":INSTALLATION,
        "state":"enrolled","allowedWorkspaces":[FIRST_WORKSPACE,SECOND_WORKSPACE],"enrollmentGeneration":2}]),
        "current machine allowance union or generation differs"
    );
    assert!(
        approvals
            == json!([
                {"approvalId":c,"requestId":LAPSED_REQUEST,"machineId":machine_id,"state":"ended",
                    "endReason":"lapsed","allowedWorkspaces":[],"baseGeneration":2,
                    "codeExpiresAt":utc(now+4)?,"machineClientId":null,"lastGrantAt":null,
                    "endedAt":utc(now+4)?,"createdAt":utc(now-1)?},
                {"approvalId":a,"requestId":FIRST_REQUEST,"machineId":machine_id,"state":"ended",
                    "endReason":"superseded","allowedWorkspaces":[FIRST_WORKSPACE],"baseGeneration":0,
                    "codeExpiresAt":utc(now+600)?,"machineClientId":a,"lastGrantAt":null,
                    "endedAt":utc(now+3)?,"createdAt":utc(now)?},
                {"approvalId":b,"requestId":SECOND_REQUEST,"machineId":machine_id,"state":"enrolled",
                    "endReason":null,"allowedWorkspaces":[SECOND_WORKSPACE],"baseGeneration":1,
                    "codeExpiresAt":utc(now+602)?,"machineClientId":b,"lastGrantAt":utc(now+3)?,
                    "endedAt":null,"createdAt":utc(now+2)?}
            ]),
        "approval fields, independent allowance snapshots or createdAt order differ"
    );

    // A second installation must add its own complete machine/approval facts;
    // observation cannot silently select just the first machine of the org.
    assert_eq!(child.set_time_ms((now + 5) * 1000).await?, 1);
    let mut other_body = approval(
        OTHER_MACHINE_REQUEST,
        "Other observed machine",
        &[SECOND_WORKSPACE],
        now + 5,
    );
    other_body["installation"] = json!(OTHER_INSTALLATION);
    other_body["peer"] = json!(OTHER_INSTALLATION);
    let d = approve(&child, &other_body).await?;
    let (all_machines, all_approvals) =
        machine_arrays(&child.observe(signed::ORGANIZATION).await?)?;
    let machine_rows = all_machines
        .as_array()
        .ok_or("multiple machine array missing")?;
    assert_eq!(
        machine_rows.len(),
        2,
        "observation did not retain both org machines"
    );
    let first_machine = machine_rows
        .iter()
        .find(|row| row.get("machineId") == Some(&json!(machine_id)))
        .ok_or("first machine disappeared from observation")?;
    let second_machine = machine_rows
        .iter()
        .find(|row| row.get("installationId") == Some(&json!(OTHER_INSTALLATION)))
        .ok_or("second installation is absent from observation")?;
    let other_machine_id = second_machine
        .get("machineId")
        .and_then(Value::as_str)
        .ok_or("second machine identity missing")?;
    message_board::RecordId::try_from(other_machine_id.to_owned())
        .map_err(|_| "second machine identity is not a canonical UUIDv7")?;
    assert!(
        other_machine_id != machine_id,
        "distinct installations shared a machine key"
    );
    // Machine rows are compared by identity; their array order is not contracted.
    assert!(
        first_machine
            == &json!({"machineId":machine_id,"installationId":INSTALLATION,
        "state":"enrolled","allowedWorkspaces":[FIRST_WORKSPACE,SECOND_WORKSPACE],"enrollmentGeneration":2}),
        "second approval rewrote the first machine facts"
    );
    assert!(
        second_machine
            == &json!({"machineId":other_machine_id,"installationId":OTHER_INSTALLATION,
        "state":"revoked","allowedWorkspaces":[SECOND_WORKSPACE],"enrollmentGeneration":0}),
        "second machine state or independent allowances differ"
    );
    let mut expected_approvals = approvals
        .as_array()
        .ok_or("earlier approval array missing")?
        .clone();
    expected_approvals.push(json!({"approvalId":d,"requestId":OTHER_MACHINE_REQUEST,
        "machineId":other_machine_id,"state":"approved","endReason":null,
        "allowedWorkspaces":[SECOND_WORKSPACE],"baseGeneration":0,
        "codeExpiresAt":utc(now+605)?,"machineClientId":null,"lastGrantAt":null,
        "endedAt":null,"createdAt":utc(now+5)?}));
    assert!(
        all_approvals == json!(expected_approvals),
        "observation omitted or mixed the independent approvals of either machine"
    );
    child.terminate_with_stdin_open().await?;
    keys.shutdown().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The first observation after real idle expiry must accept automatic audit origin and expose the fenced machine state."
)]
#[tokio::test]
async fn private_observe_first_after_idle_expiry_accepts_automatic_records_and_projects_the_fence()
-> TestResult {
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::Builder::new().prefix("sim-").tempdir()?;
    let mut child = LauncherChild::start(
        &root.path().join("data"),
        &root.path().join("s.sock"),
        &keys,
    )
    .await?;
    let now = signed::NOW_SECONDS;
    provision(
        &child,
        signed::ORGANIZATION,
        &signed::stamp(signed::ORGANIZATION, now)?,
    )
    .await?;
    create_workspaces(&child).await?;
    let a = approve(&child, &approval(FIRST_REQUEST, "Idle observed", &[], now)).await?;
    assert!(
        token(&child, &a).await? == grant(&a, now, None),
        "idle fixture first grant differs"
    );
    let idle_at = now + 30 * 24 * 60 * 60;
    assert_eq!(child.set_time_ms((idle_at - 60) * 1000).await?, 1);
    let b = approve(
        &child,
        &approval(
            SECOND_REQUEST,
            "Pending observed",
            &[FIRST_WORKSPACE],
            idle_at - 60,
        ),
    )
    .await?;
    assert_eq!(child.set_time_ms(idle_at * 1000).await?, 1);
    assert!(
        token(&child, &a).await? == json!({"result":"refuse","reason":{"kind":"idleExpired"}}),
        "exact idle boundary did not expire the old login"
    );

    // This is deliberately the FIRST observe call in this test. The old
    // Automatic-origin decoder fails here before any new-array assertion runs.
    let observed = child.observe(signed::ORGANIZATION).await?;
    let expiry_records = observed
        .records
        .iter()
        .filter(|record| record.created_at_millis == idle_at * 1000)
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "automatic expiry record encoding failed")?;
    assert!(
        expiry_records
            == vec![
                json!({"kind":"machineExpired","requestId":format!("{a}:machineExpired:{a}"),
            "targetId":a,"createdAtMillis":idle_at*1000}),
                json!({"kind":"allowanceChanged","requestId":format!("{a}:allowanceChanged:{FIRST_WORKSPACE}"),
            "targetId":FIRST_WORKSPACE,"createdAtMillis":idle_at*1000})
            ],
        "real automatic expiry records are absent or differ in observation"
    );
    let (machines, approvals) = machine_arrays(&observed)?;
    let machine_id = machines
        .pointer("/0/machineId")
        .and_then(Value::as_str)
        .ok_or("idle observation machine identity is missing")?;
    assert!(
        machines
            == json!([{"machineId":machine_id,"installationId":INSTALLATION,
        "state":"revoked","allowedWorkspaces":[],"enrollmentGeneration":2}]),
        "idle observation omitted allowance withdrawal or generation fence"
    );
    assert!(
        approvals
            == json!([
                {"approvalId":a,"requestId":FIRST_REQUEST,"machineId":machine_id,"state":"ended",
                    "endReason":"idle","allowedWorkspaces":[],"baseGeneration":0,
                    "codeExpiresAt":utc(now+600)?,"machineClientId":a,"lastGrantAt":null,
                    "endedAt":utc(idle_at)?,"createdAt":utc(now)?},
                {"approvalId":b,"requestId":SECOND_REQUEST,"machineId":machine_id,"state":"approved",
                    "endReason":null,"allowedWorkspaces":[FIRST_WORKSPACE],"baseGeneration":1,
                    "codeExpiresAt":utc(idle_at+540)?,"machineClientId":null,"lastGrantAt":null,
                    "endedAt":null,"createdAt":utc(idle_at-60)?}
            ]),
        "idle/pending snapshots or canonical nullable lifecycle times differ"
    );
    assert!(
        token(&child, &b).await? == json!({"result":"refuse","reason":{"kind":"superseded"}}),
        "live pending code crossed the observed idle fence"
    );
    assert!(
        child.observe(signed::ORGANIZATION).await? == observed,
        "refused stale first grant changed the observation"
    );
    child.terminate_with_stdin_open().await?;
    keys.shutdown().await
}
