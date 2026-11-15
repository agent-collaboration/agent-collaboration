//! Machine rows observed through the established launcher IPC after real HTTP decisions.
use super::{Bridge, ConformanceResult, Value, json, signed};
use std::path::Path;

const INSTALLATION: &str = "019f0000-0000-7000-8000-00000000e101";
const INSTALLATION_TWO: &str = "019f0000-0000-7000-8000-00000000e104";
const WORKSPACE_ONE: &str = "019f0000-0000-7000-8000-00000000e102";
const WORKSPACE_TWO: &str = "019f0000-0000-7000-8000-00000000e103";
const REQUEST_ONE: &str = "019f0000-0000-7000-8000-00000000e1f3";
const REQUEST_TWO: &str = "019f0000-0000-7000-8000-00000000e1f2";
const REQUEST_THREE: &str = "019f0000-0000-7000-8000-00000000e1f1";
const REQUEST_FOUR: &str = "019f0000-0000-7000-8000-00000000e1f4";
const DEVICE: &str = "/internal/device-login-decisions";
const TOKEN: &str = "/internal/machine-token-decisions";

fn write_plan(path: &Path, keys: &signed::SignInKeys) -> ConformanceResult<()> {
    std::fs::write(
        path,
        serde_json::to_vec(&json!({"textBindings":{
            "CENTRAL_SIGN_IN_ISSUER":signed::ISSUER,
            "CENTRAL_SIGN_IN_AUDIENCE":signed::AUDIENCE,
            "CENTRAL_SIGN_IN_JWKS_URL":"https://signin.test/.well-known/jwks.json",
            "CENTRAL_ADMIN_AUDIENCE":signed::ADMIN_AUDIENCE,
            "CENTRAL_PUBLIC_ORIGIN":signed::PUBLIC_ORIGIN,
            "CENTRAL_SIGN_IN_SERVICE_ID":"019f0000-0000-7000-8000-00000000d290"
        },"jwksFixtureUrl":keys.url}))?,
    )?;
    Ok(())
}

async fn post(bridge: &Bridge, path: &str, body: &Value) -> ConformanceResult<Value> {
    let response = bridge
        .client
        .post(format!("{}/private{path}", bridge.origin))
        .json(body)
        .send()
        .await?;
    assert_eq!(response.status().as_u16(), 200);
    Ok(response.json().await?)
}

async fn create_workspaces(bridge: &Bridge) -> ConformanceResult<()> {
    // Existing signed setup is a directory precondition, never seeded machine state.
    for revision in [
        json!({"kind":"organization","organizationId":signed::ORGANIZATION,"revision":3,"state":"active","permissionPolicyDigest":signed::POLICY}),
        json!({"kind":"person","organizationId":signed::ORGANIZATION,"personId":signed::OWNER,"subjectId":signed::OWNER_SUBJECT,"revision":4,"state":"active"}),
    ] {
        post(
            bridge,
            "/internal/organization-authorization-revisions",
            &revision,
        )
        .await?;
    }
    let owner = signed::Person::owner(signed::ORGANIZATION, signed::NOW_SECONDS)?;
    for (operation, name) in [
        (WORKSPACE_ONE, "Observation one"),
        (WORKSPACE_TWO, "Observation two"),
    ] {
        let proof = owner.headers(
            signed::CREATE,
            &signed::create_intent(signed::ORGANIZATION, operation, name),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
            signed::NOW_SECONDS,
        )?;
        let mut request = bridge
            .client
            .post(format!("{}/private{}", bridge.origin, signed::CREATE))
            .json(&signed::create_body(signed::ORGANIZATION, operation, name));
        for (header, value) in proof {
            request = request.header(header, value);
        }
        let response = request.send().await?;
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(
            response.json::<Value>().await?,
            json!({"result":"applied","workspace":{"workspaceId":operation,"name":name,"description":"","state":"active"}})
        );
    }
    Ok(())
}

fn approval(request: &str, workspaces: &[&str], now: i64, deadline: i64) -> Value {
    json!({"organizationId":signed::ORGANIZATION,"approver":signed::OWNER,
        "permissionChecked":"machine:enroll","requestId":request,"installation":INSTALLATION,
        "peer":INSTALLATION,"label":"Observed machine","allowedWorkspaces":workspaces,
        "approverSecondFactorAt":now-60,"codeExpiresAt":deadline})
}

async fn approve(
    bridge: &Bridge,
    request: &str,
    workspaces: &[&str],
    now: i64,
    deadline: i64,
) -> ConformanceResult<String> {
    let answer = post(
        bridge,
        DEVICE,
        &approval(request, workspaces, now, deadline),
    )
    .await?;
    let id = answer
        .get("enrollmentId")
        .and_then(Value::as_str)
        .ok_or("approval ID missing")?;
    let parsed = uuid::Uuid::parse_str(id)?;
    assert_eq!(parsed.get_version_num(), 7);
    assert_eq!(parsed.to_string(), id);
    assert_eq!(
        answer,
        json!({"result":"approved","enrollmentId":id,"label":"Observed machine","allowedWorkspaces":workspaces})
    );
    Ok(id.to_owned())
}

async fn token(bridge: &Bridge, enrollment: &str) -> ConformanceResult<Value> {
    post(
        bridge,
        TOKEN,
        &json!({"organizationId":signed::ORGANIZATION,
        "enrollmentId":enrollment,"clientId":enrollment}),
    )
    .await
}

fn timestamp(seconds: i64) -> ConformanceResult<String> {
    Ok(chrono::DateTime::from_timestamp(seconds, 0)
        .ok_or("expected fixture timestamp is out of range")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn expected_approval(
    enrollment: &str,
    request: &str,
    machine: &str,
    workspaces: &[&str],
    generation: i64,
    times: [i64; 2],
) -> ConformanceResult<Value> {
    Ok(
        json!({"approvalId":enrollment,"requestId":request,"machineId":machine,
        "state":"approved","endReason":null,"allowedWorkspaces":workspaces,
        "baseGeneration":generation,"codeExpiresAt":timestamp(times[1])?,
        "machineClientId":null,"lastGrantAt":null,"endedAt":null,
        "createdAt":timestamp(times[0])?}),
    )
}

fn expected_machine(machine: &str, state: &str, workspaces: &[&str], generation: i64) -> Value {
    json!({"machineId":machine,"installationId":INSTALLATION,"state":state,
        "allowedWorkspaces":workspaces,"enrollmentGeneration":generation})
}

fn machine_id(observation: &Value) -> ConformanceResult<String> {
    let rows = observation
        .get("machines")
        .and_then(Value::as_array)
        .ok_or("machines missing")?;
    assert_eq!(rows.len(), 1);
    let id = rows[0]
        .get("machineId")
        .and_then(Value::as_str)
        .ok_or("machine ID missing")?;
    let parsed = uuid::Uuid::parse_str(id)?;
    assert_eq!(parsed.get_version_num(), 7);
    assert_eq!(parsed.to_string(), id);
    Ok(id.to_owned())
}

fn assert_no_credentials(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    !key.to_ascii_lowercase().contains("credential")
                        && !key.to_ascii_lowercase().contains("token")
                        && !key.to_ascii_lowercase().contains("secret"),
                    "observation must not expose credential fields"
                );
                assert_no_credentials(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_credentials),
        _ => {}
    }
}

fn assert_rows(observation: &Value, machine: Value, approvals: Vec<Value>) {
    // Opaque comparisons keep the enclosing compact statement out of panic output.
    assert!(
        observation.get("machines") == Some(&json!([machine])),
        "machine observation differs from the exact requested fields"
    );
    assert!(
        observation.get("machineApprovals") == Some(&json!(approvals)),
        "approval observation differs from exact lifecycle fields or createdAt order"
    );
    assert_no_credentials(observation);
}

#[tokio::test]
async fn bridge_observes_machine_approval_lifecycle_fields_in_created_at_order()
-> ConformanceResult<()> {
    // Arrange: real bridge, signed provisioning and signed workspace preconditions.
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::tempdir()?;
    let plan = root.path().join("machine-observe-plan.json");
    write_plan(&plan, &keys)?;
    let mut bridge = Bridge::start(&plan).await?;
    let start = signed::NOW_SECONDS;
    bridge
        .provision(
            signed::ORGANIZATION,
            &signed::stamp(signed::ORGANIZATION, start)?,
        )
        .await?;
    create_workspaces(&bridge).await?;

    // Act/Assert: approval includes its own allowance list and explicit absent times.
    let first = approve(&bridge, REQUEST_ONE, &[WORKSPACE_ONE], start, start + 600).await?;
    let observed = bridge.observe(signed::ORGANIZATION).await?;
    let machine = machine_id(&observed)?;
    let mut first_row = expected_approval(
        &first,
        REQUEST_ONE,
        &machine,
        &[WORKSPACE_ONE],
        0,
        [start, start + 600],
    )?;
    assert_rows(
        &observed,
        expected_machine(&machine, "revoked", &[WORKSPACE_ONE], 0),
        vec![first_row.clone()],
    );
    bridge.set_time((start + 1) * 1000, 1).await?;
    assert_eq!(
        token(&bridge, &first).await?,
        json!({"result":"grant","installation":INSTALLATION,
        "enrollmentId":first,"decidedAt":start+1,"grantExpiresAt":start+601})
    );
    first_row["state"] = json!("enrolled");
    first_row["machineClientId"] = json!(first);
    first_row["lastGrantAt"] = json!(timestamp(start + 1)?);
    assert_rows(
        &bridge.observe(signed::ORGANIZATION).await?,
        expected_machine(&machine, "enrolled", &[WORKSPACE_ONE], 1),
        vec![first_row.clone()],
    );

    bridge.set_time((start + 2) * 1000, 1).await?;
    let second = approve(
        &bridge,
        REQUEST_TWO,
        &[WORKSPACE_TWO],
        start + 2,
        start + 602,
    )
    .await?;
    let mut second_row = expected_approval(
        &second,
        REQUEST_TWO,
        &machine,
        &[WORKSPACE_TWO],
        1,
        [start + 2, start + 602],
    )?;
    assert_rows(
        &bridge.observe(signed::ORGANIZATION).await?,
        expected_machine(&machine, "enrolled", &[WORKSPACE_ONE, WORKSPACE_TWO], 1),
        vec![first_row.clone(), second_row.clone()],
    );
    bridge.set_time((start + 3) * 1000, 1).await?;
    assert_eq!(
        token(&bridge, &second).await?,
        json!({"result":"grant","installation":INSTALLATION,
        "enrollmentId":second,"decidedAt":start+3,"grantExpiresAt":start+603,
        "superseded":{"enrollmentId":first,"clientId":first}})
    );
    first_row["state"] = json!("ended");
    first_row["endReason"] = json!("superseded");
    first_row["lastGrantAt"] = Value::Null;
    first_row["endedAt"] = json!(timestamp(start + 3)?);
    second_row["state"] = json!("enrolled");
    second_row["machineClientId"] = json!(second);
    second_row["lastGrantAt"] = json!(timestamp(start + 3)?);

    bridge.set_time((start + 4) * 1000, 1).await?;
    assert_eq!(
        token(&bridge, &second).await?,
        json!({"result":"grant","installation":INSTALLATION,
        "enrollmentId":second,"decidedAt":start+4,"grantExpiresAt":start+604,
        "superseded":{"enrollmentId":first,"clientId":first}})
    );
    second_row["lastGrantAt"] = json!(timestamp(start + 4)?);
    assert_rows(
        &bridge.observe(signed::ORGANIZATION).await?,
        expected_machine(&machine, "enrolled", &[WORKSPACE_ONE, WORKSPACE_TWO], 2),
        vec![first_row.clone(), second_row.clone()],
    );

    // C is minted after A/B but stored at an earlier applying time. This makes
    // createdAt ordering distinguishable from generated approval-ID ordering.
    bridge.set_time((start - 1) * 1000, 1).await?;
    let third = approve(&bridge, REQUEST_THREE, &[], start - 1, start + 6).await?;
    let mut third_row = expected_approval(
        &third,
        REQUEST_THREE,
        &machine,
        &[],
        2,
        [start - 1, start + 6],
    )?;
    bridge.set_time((start + 6) * 1000, 1).await?;
    assert_eq!(
        token(&bridge, &third).await?,
        json!({"result":"refuse","reason":{"kind":"enrollmentNotActive"}})
    );
    third_row["state"] = json!("ended");
    third_row["endReason"] = json!("lapsed");
    third_row["endedAt"] = json!(timestamp(start + 6)?);
    assert_rows(
        &bridge.observe(signed::ORGANIZATION).await?,
        expected_machine(&machine, "enrolled", &[WORKSPACE_ONE, WORKSPACE_TWO], 2),
        vec![third_row.clone(), first_row.clone(), second_row.clone()],
    );

    // A distinct installation creates another owning machine. Observe the whole
    // organization, not just the machine used by the preceding lifecycle stages.
    bridge.set_time((start + 7) * 1000, 1).await?;
    let mut another = approval(REQUEST_FOUR, &[WORKSPACE_TWO], start + 7, start + 607);
    another["installation"] = json!(INSTALLATION_TWO);
    another["peer"] = json!(INSTALLATION_TWO);
    let approved = post(&bridge, DEVICE, &another).await?;
    let fourth = approved
        .get("enrollmentId")
        .and_then(Value::as_str)
        .ok_or("second machine approval ID missing")?;
    assert_eq!(uuid::Uuid::parse_str(fourth)?.get_version_num(), 7);
    assert_eq!(
        approved,
        json!({"result":"approved","enrollmentId":fourth,
        "label":"Observed machine","allowedWorkspaces":[WORKSPACE_TWO]})
    );
    let observed = bridge.observe(signed::ORGANIZATION).await?;
    let mut actual_machines = observed
        .get("machines")
        .and_then(Value::as_array)
        .ok_or("machines missing")?
        .clone();
    assert_eq!(actual_machines.len(), 2);
    let second_machine = actual_machines
        .iter()
        .find(|row| row.get("installationId") == Some(&json!(INSTALLATION_TWO)))
        .and_then(|row| row.get("machineId"))
        .and_then(Value::as_str)
        .ok_or("second installation's owning machine missing")?;
    let second_machine = second_machine.to_owned();
    let parsed = uuid::Uuid::parse_str(&second_machine)?;
    assert_eq!(parsed.get_version_num(), 7);
    assert_eq!(parsed.to_string(), second_machine);
    assert_ne!(second_machine, machine);
    let mut expected_machines = vec![
        expected_machine(&machine, "enrolled", &[WORKSPACE_ONE, WORKSPACE_TWO], 2),
        json!({"machineId":second_machine,"installationId":INSTALLATION_TWO,"state":"revoked",
            "allowedWorkspaces":[WORKSPACE_TWO],"enrollmentGeneration":0}),
    ];
    // Machines have no promised order; cardinality plus complete identity-sorted
    // rows catches omission, duplication and leaking allowances across machines.
    actual_machines.sort_by_key(|row| row["machineId"].as_str().map(str::to_owned));
    expected_machines.sort_by_key(|row| row["machineId"].as_str().map(str::to_owned));
    assert!(
        actual_machines == expected_machines,
        "observe must return both complete independent machine rows"
    );
    let fourth_row = expected_approval(
        fourth,
        REQUEST_FOUR,
        &second_machine,
        &[WORKSPACE_TWO],
        0,
        [start + 7, start + 607],
    )?;
    assert!(
        observed.get("machineApprovals")
            == Some(&json!([third_row, first_row, second_row, fourth_row])),
        "all approvals must retain their owning machine, independent allowances and createdAt order"
    );
    assert_no_credentials(&observed);
    bridge.close().await?;
    keys.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn bridge_first_observation_after_real_idle_expiry_accepts_automatic_records()
-> ConformanceResult<()> {
    let keys = signed::SignInKeys::start().await?;
    let root = tempfile::tempdir()?;
    let plan = root.path().join("machine-idle-observe-plan.json");
    write_plan(&plan, &keys)?;
    let mut bridge = Bridge::start(&plan).await?;
    let start = signed::NOW_SECONDS;
    bridge
        .provision(
            signed::ORGANIZATION,
            &signed::stamp(signed::ORGANIZATION, start)?,
        )
        .await?;
    create_workspaces(&bridge).await?;
    let first = approve(&bridge, REQUEST_ONE, &[WORKSPACE_ONE], start, start + 600).await?;
    assert_eq!(
        token(&bridge, &first).await?,
        json!({"result":"grant","installation":INSTALLATION,
        "enrollmentId":first,"decidedAt":start,"grantExpiresAt":start+600})
    );
    let idle_at = start + 30 * 24 * 60 * 60;
    bridge.set_time((idle_at - 60) * 1000, 1).await?;
    let pending = approve(
        &bridge,
        REQUEST_TWO,
        &[WORKSPACE_TWO],
        idle_at - 60,
        idle_at + 540,
    )
    .await?;
    bridge.set_time(idle_at * 1000, 1).await?;
    assert_eq!(
        token(&bridge, &first).await?,
        json!({"result":"refuse","reason":{"kind":"idleExpired"}})
    );
    assert_eq!(
        token(&bridge, &pending).await?,
        json!({"result":"refuse","reason":{"kind":"superseded"}})
    );

    // This is deliberately the first observe call: old Automatic-origin decoding
    // must fail here before any missing-machine-field assertion can mask it.
    let observed = bridge.observe(signed::ORGANIZATION).await?;
    let records = observed
        .get("records")
        .and_then(Value::as_array)
        .ok_or("records missing")?;
    let mut automatic: Vec<_> = records
        .iter()
        .filter(|record| record.get("createdAtMillis") == Some(&json!(idle_at * 1000)))
        .cloned()
        .collect();
    let mut expected_records = vec![
        json!({"kind":"machineExpired","requestId":format!("{first}:machineExpired:{first}"),"targetId":first,"createdAtMillis":idle_at*1000}),
        json!({"kind":"allowanceChanged","requestId":format!("{first}:allowanceChanged:{WORKSPACE_ONE}"),"targetId":WORKSPACE_ONE,"createdAtMillis":idle_at*1000}),
        json!({"kind":"allowanceChanged","requestId":format!("{first}:allowanceChanged:{WORKSPACE_TWO}"),"targetId":WORKSPACE_TWO,"createdAtMillis":idle_at*1000}),
    ];
    // Record order is not the approval createdAt ordering contract. Compare their
    // literal content after an independent request-key sort, without dumping IPC.
    automatic.sort_by_key(|record| record["requestId"].as_str().map(str::to_owned));
    expected_records.sort_by_key(|record| record["requestId"].as_str().map(str::to_owned));
    assert!(
        automatic == expected_records,
        "idle observation must retain the actual automatic expiry and withdrawal records"
    );
    let machine = machine_id(&observed)?;
    let mut first_row = expected_approval(
        &first,
        REQUEST_ONE,
        &machine,
        &[WORKSPACE_ONE],
        0,
        [start, start + 600],
    )?;
    first_row["state"] = json!("ended");
    first_row["endReason"] = json!("idle");
    first_row["machineClientId"] = json!(first);
    first_row["endedAt"] = json!(timestamp(idle_at)?);
    let pending_row = expected_approval(
        &pending,
        REQUEST_TWO,
        &machine,
        &[WORKSPACE_TWO],
        1,
        [idle_at - 60, idle_at + 540],
    )?;
    assert_rows(
        &observed,
        expected_machine(&machine, "revoked", &[], 2),
        vec![first_row, pending_row],
    );
    bridge.close().await?;
    keys.shutdown().await?;
    Ok(())
}
