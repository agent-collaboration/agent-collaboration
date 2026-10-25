//! Portable engine bootstrap on real DO SQL, including owning process restart.
#![forbid(unsafe_code)]
#[allow(
    dead_code,
    reason = "The established process harness also serves socket scenarios."
)]
#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
mod workerd_process;

use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, owner_assertion};
use collaboration_central_core::{
    BootstrapCommand, BootstrapHistory, BootstrapProbe, BootstrapProbeReply, BootstrapReply,
    EngineSchema, HistoryCorruption,
};
use durable_steps::StoreError;
use fleet_sync_protocol::{
    CopyStatusAnswer, CopyUnavailableReason, DatabaseId, ItemStatusState, ReplicationAnswer,
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use workerd_process::{WorkerdProcess, WorkerdSettings};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply<Output = BootstrapReply> {
    result: Result<Output, StoreError>,
    reply_loss_observed: bool,
}
struct Client {
    http: reqwest::Client,
    url: String,
}
impl Client {
    async fn enrollment(&self, command: serde_json::Value) -> ConformanceResult<serde_json::Value> {
        let reply: Reply<serde_json::Value> = self
            .http
            .post(&self.url)
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&json!({"fixture":"enrollment","command":command,"fault":null}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(reply.result?)
    }
    async fn raw(
        &self,
        fixture: &str,
        command: BootstrapCommand,
        lose_reply: bool,
    ) -> ConformanceResult<Reply> {
        let fault = if lose_reply {
            Some("loseRollbackReply")
        } else {
            None
        };
        Ok(self
            .http
            .post(&self.url)
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&json!({"fixture":fixture,"command":{"kind":"bootstrap","value":command},"fault":fault}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    async fn call(
        &self,
        fixture: &str,
        command: BootstrapCommand,
    ) -> ConformanceResult<BootstrapReply> {
        Ok(self.raw(fixture, command, false).await?.result?)
    }
    async fn snapshot(
        &self,
        fixture: &str,
    ) -> ConformanceResult<(Vec<String>, Vec<BootstrapHistory>)> {
        match self.call(fixture, BootstrapCommand::Snapshot).await? {
            BootstrapReply::Snapshot { tables, history } => Ok((tables, history)),
            other => Err(format!("snapshot returned {other:?}").into()),
        }
    }
    async fn initialize(
        &self,
        fixture: &str,
        schema: EngineSchema,
        expected: usize,
    ) -> ConformanceResult<()> {
        match self
            .call(fixture, BootstrapCommand::Initialize { schema })
            .await?
        {
            BootstrapReply::Initialized { applied } if applied == expected => Ok(()),
            other => {
                Err(format!("bootstrap did not apply {expected} migrations: {other:?}").into())
            }
        }
    }
}

fn schemas() -> [(&'static str, EngineSchema, usize, Option<BootstrapProbe>); 4] {
    let mailbox_migration_count = fleet_sync_delivery::DELIVERY_MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .count();
    [
        (
            "registry",
            EngineSchema::Registry,
            8,
            Some(BootstrapProbe::Registry),
        ),
        ("copy", EngineSchema::Copy, 3, Some(BootstrapProbe::Copy)),
        ("session", EngineSchema::Session, 4, None),
        (
            "mailbox",
            EngineSchema::Mailbox,
            mailbox_migration_count,
            Some(BootstrapProbe::Mailbox),
        ),
    ]
}
async fn owner_gate(client: &Client) -> ConformanceResult<()> {
    use collaboration_central_conformance::{
        FIXTURE_KID, access_client_id, fixture_claims, sign_assertion,
    };
    let machine = sign_assertion(
        &fixture_claims(
            collaboration_central_core::AccessCaller::Machine {
                client_id: access_client_id("bootstrap-non-owner")?,
            },
            true,
        ),
        FIXTURE_KID,
    )?;
    for (label, assertion) in [
        ("missing-assertion", None),
        ("machine-assertion", Some(machine)),
    ] {
        let request = client.http.post(&client.url)
            .json(&json!({"fixture":"registry","command":{"kind":"bootstrap","value":{"kind":"initialize","value":{"schema":"registry"}}},"fault":null}));
        let request = match assertion {
            Some(assertion) => request.header("Cf-Access-Jwt-Assertion", assertion),
            None => request,
        };
        eprintln!("BOOTSTRAP_OWNER_GATE {label} send");
        let response = request
            .send()
            .await
            .map_err(|error| format!("bootstrap owner-gate {label} send: {error}"))?;
        eprintln!(
            "BOOTSTRAP_OWNER_GATE {label} status={} version={:?} content_length={:?}",
            response.status(),
            response.version(),
            response.content_length()
        );
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Err(format!("a non-owner reached Bootstrap: {}", response.status()).into());
        }
    }
    // Authentication must also precede parsing a malformed, non-UTF-8 body.
    // The upload spans multiple buffers so an early refusal cannot silently
    // leave a partially read request on this reused connection.
    eprintln!("BOOTSTRAP_OWNER_GATE malformed-body send");
    let response = client
        .http
        .post(&client.url)
        .header("Content-Type", "application/json")
        .body(vec![0xff_u8; 64 * 1024])
        .send()
        .await
        .map_err(|error| format!("bootstrap owner-gate malformed-body send: {error}"))?;
    if response.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Err(format!(
            "a malformed non-owner body bypassed the gate: {}",
            response.status()
        )
        .into());
    }
    eprintln!(
        "BOOTSTRAP_OWNER_GATE malformed-body status={}",
        response.status()
    );
    // The following owner initialization still has to apply all eight migrations,
    // so neither refused request may have initialized the actual fixture store.
    println!("BOOTSTRAP_CASE missing_and_valid_machine_admin_assertions_cannot_initialize passed");
    Ok(())
}

async fn first_process(client: &Client) -> ConformanceResult<String> {
    owner_gate(client).await?;
    for (fixture, schema, count, probe) in schemas() {
        client.initialize(fixture, schema, count).await?;
        client.initialize(fixture, schema, 0).await?;
        let (tables, history) = client.snapshot(fixture).await?;
        if history.len() != count
            || !tables
                .iter()
                .any(|name| name == "central_schema_migrations")
        {
            return Err("bootstrap history/table readback missing".into());
        }
        if let Some(schema) = probe {
            require_probe(
                client
                    .call(fixture, BootstrapCommand::Probe { schema })
                    .await?,
            )?;
        }
        println!("BOOTSTRAP_CASE {fixture}_public_corpus_and_real_sql passed");
    }
    client
        .call("lateConflict", BootstrapCommand::PrepareLateConflict)
        .await?;
    let before = client.snapshot("lateConflict").await?;
    if !before.0.iter().any(|name| name == "_fleet_sync_install") || !before.1.is_empty() {
        return Err("late-conflict fixture did not reach its durable precondition".into());
    }
    let failed = client
        .raw(
            "lateConflict",
            BootstrapCommand::Initialize {
                schema: EngineSchema::Copy,
            },
            false,
        )
        .await?
        .result;
    let Err(StoreError::StepRollback { failure }) = failed else {
        return Err("late SQL conflict lost the confirmed migration rollback marker".into());
    };
    let durable_steps::OperationFailure::Refused { code, detail } = failure.as_ref() else {
        return Err("late SQL conflict lost its typed central failure context".into());
    };
    let diagnostic: serde_json::Value = serde_json::from_str(detail)?;
    if code != "central_schema_migration"
        || diagnostic.get("corpus") != Some(&json!("fleet-sync-receiver"))
        || diagnostic.get("version") != Some(&json!(3))
        || !diagnostic
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|reason| reason.contains("_fleet_sync_install"))
        || diagnostic.get("at") != Some(&json!(1_900_000_000_000_i64))
    {
        return Err("late SQL conflict lost its actual corpus/version/cause/time".into());
    }
    let mut expected = before;
    expected.0.push("central_schema_migration_failures".into());
    expected.0.sort();
    if client.snapshot("lateConflict").await? != expected {
        return Err(
            "late SQL conflict leaked earlier schema/history beyond the separate diagnostic table"
                .into(),
        );
    }
    println!("BOOTSTRAP_CASE late_sql_conflict_rolls_back_schema_and_history passed");

    let database = DatabaseId::try_from("019f0000-0000-7000-8000-000000000603".to_owned())?;
    let answer = client
        .call("damaged", BootstrapCommand::CopyStatus { database })
        .await?;
    if !matches!(answer, BootstrapReply::CopyStatus(answer) if matches!(*answer, ReplicationAnswer::CopyUnavailable { reason: CopyUnavailableReason::Damaged { .. } }))
    {
        return Err("real receiver SQL error was not rollback-confirmed unavailable".into());
    }
    client.initialize("damaged", EngineSchema::Copy, 3).await?;
    if !matches!(client.call("damaged", BootstrapCommand::CopyStatus { database }).await?, BootstrapReply::CopyStatus(answer) if matches!(*answer, ReplicationAnswer::CopyStatus(CopyStatusAnswer::NotInitialized)))
    {
        return Err("receiver did not recover after real bootstrap".into());
    }
    let uncertain = client
        .raw("uncertain", BootstrapCommand::CopyStatus { database }, true)
        .await?;
    if !uncertain.reply_loss_observed
        || !matches!(uncertain.result, Ok(BootstrapReply::CopyStatus(answer)) if matches!(*answer, ReplicationAnswer::OutcomeUnknown { .. }))
    {
        return Err("a lost real rollback reply was promoted to no-effect".into());
    }
    println!("BOOTSTRAP_CASE receiver_rollback_and_lost_boundary_reply_remain_distinct passed");
    enrollment_cases(client).await
}

const ENROLLED_PEER: &str = "019f0000-0000-7000-8000-000000000d01";

fn enrollment_request() -> serde_json::Value {
    json!({"installation":"550e8400-e29b-41d4-a716-446655440000", "peer":ENROLLED_PEER, "label":"DO original label", "access_client_id":"do-enrollment-client", "credential_hash":"do-enrollment-hash", "expires_at":"2033-05-18T03:33:20Z"})
}
fn enrolled_peer(response: &serde_json::Value) -> ConformanceResult<String> {
    if response.get("kind") != Some(&json!("machineEnrolled"))
        || response.pointer("/value/expiresAt") != Some(&json!("2033-05-18T03:33:20Z"))
        || response
            .get("value")
            .and_then(serde_json::Value::as_object)
            .map(|v| v.len())
            != Some(4)
    {
        return Err(
            "enrollment did not return peer, expiry, authoritative receiver and relay peer".into(),
        );
    }
    if response.pointer("/value/authoritativeReceiver")
        != Some(&json!("019f0000-0000-7000-8000-000000000010"))
        || response.pointer("/value/relayPeer")
            != Some(&json!("019f0000-0000-7000-8000-000000005240"))
    {
        return Err("enrollment returned the wrong configured receiver or relay peer".into());
    }
    let peer = response
        .pointer("/value/peer")
        .and_then(serde_json::Value::as_str)
        .ok_or("enrollment omitted peer")?;
    let peer_id = fleet_sync_protocol::PeerId::try_from(peer.to_owned())?;
    if peer_id.to_string() != ENROLLED_PEER {
        return Err("enrollment did not retain the supplied installation peer".into());
    }
    Ok(peer.to_owned())
}
async fn enrollment_cases(client: &Client) -> ConformanceResult<String> {
    client
        .initialize("enrollment", EngineSchema::Registry, 8)
        .await?;
    let request = enrollment_request();
    let expected = enrolled_peer(
        &client
            .enrollment(json!({"kind":"enroll","value":request}))
            .await?,
    )?;
    let (left, right) = tokio::join!(
        client.enrollment(json!({"kind":"enroll","value":request})),
        client.enrollment(json!({"kind":"enroll","value":request}))
    );
    if enrolled_peer(&left?)? != expected || enrolled_peer(&right?)? != expected {
        return Err("concurrent DO enrollment replaced peer".into());
    }
    println!("BOOTSTRAP_CASE v4_enrollment_and_concurrent_updates_keep_one_v7_peer passed");

    let authentication = json!({"kind":"authenticate","value":{"access_client_id":"do-enrollment-client","credential_hash":"do-enrollment-hash"}});
    let admitted = client.enrollment(authentication.clone()).await?;
    if admitted.pointer("/Ok/peer") != Some(&json!(expected))
        || admitted.pointer("/Ok/origin")
            != Some(
                &json!({"installation":"550e8400-e29b-41d4-a716-446655440000","machine":"DO original label"}),
            )
    {
        return Err("DO authentication did not resolve exact enrollment origin".into());
    }
    let mut relabeled = request.clone();
    *relabeled.get_mut("label").ok_or("fixture omitted label")? = json!("DO updated label");
    if enrolled_peer(
        &client
            .enrollment(json!({"kind":"enroll","value":relabeled}))
            .await?,
    )? != expected
    {
        return Err("relabel changed peer".into());
    }
    let after = client.enrollment(authentication).await?;
    if admitted.pointer("/Ok/origin/machine") != Some(&json!("DO original label"))
        || after.pointer("/Ok/origin/machine") != Some(&json!("DO updated label"))
    {
        return Err("admitted origin was relabeled or new origin stale".into());
    }
    let wrong_access = client.enrollment(json!({"kind":"authenticate","value":{"access_client_id":"wrong-client","credential_hash":"do-enrollment-hash"}})).await?;
    if wrong_access != json!({"Err":"wrongKind"}) {
        return Err("Access and credential were not bound to the same DO row".into());
    }
    println!(
        "BOOTSTRAP_CASE enrollment_authentication_binds_client_and_captures_exact_origin passed"
    );

    for (client_id, hash, refusal) in [
        ("do-enrollment-client", "unused", "accessClientInUse"),
        ("unused", "do-enrollment-hash", "credentialHashInUse"),
    ] {
        let mut conflicting = request.clone();
        *conflicting
            .get_mut("installation")
            .ok_or("fixture omitted installation")? = json!("550e8400-e29b-41d4-a716-446655440001");
        *conflicting.get_mut("peer").ok_or("fixture omitted peer")? =
            json!("019f0000-0000-7000-8000-000000000d02");
        *conflicting
            .get_mut("access_client_id")
            .ok_or("fixture omitted client")? = json!(client_id);
        *conflicting
            .get_mut("credential_hash")
            .ok_or("fixture omitted hash")? = json!(hash);
        if client
            .enrollment(json!({"kind":"enroll","value":conflicting}))
            .await?
            != json!({"kind":"refused","value":refusal})
        {
            return Err("DO unique identity conflict was not explicitly refused".into());
        }
    }
    println!("BOOTSTRAP_CASE enrollment_client_and_hash_conflicts_are_atomic_refusals passed");
    Ok(expected)
}
fn require_probe(reply: BootstrapReply) -> ConformanceResult<()> {
    let BootstrapReply::Probed(probe) = reply else {
        return Err("library rule probe missing".into());
    };
    match *probe {
        BootstrapProbeReply::Registry(report)
            if report.peer.to_string() == "019f0000-0000-7000-8000-000000000601"
                && report.session_id.to_string() == "019f0000-0000-7000-8000-000000000602"
                && report.attach_seq.value() == 1
                && report.support.max_version == 7 =>
        {
            Ok(())
        }
        BootstrapProbeReply::Copy(answer) if matches!(&*answer, CopyStatusAnswer::Status(status) if status.database.to_string() == "019f0000-0000-7000-8000-000000000603" && status.fence.value() == 1 && status.acked.sequence.value() == 0) => {
            Ok(())
        }
        BootstrapProbeReply::Mailbox(query)
            if query.statuses.len() == 1
                && query
                    .statuses
                    .iter()
                    .all(|status| matches!(status.state, ItemStatusState::Unknown)) =>
        {
            Ok(())
        }
        other => Err(format!("library rule readback mismatch: {other:?}").into()),
    }
}
async fn second_process(client: &Client, expected_peer: &str) -> ConformanceResult<()> {
    let request = enrollment_request();
    if enrolled_peer(
        &client
            .enrollment(json!({"kind":"enroll","value":request}))
            .await?,
    )? != expected_peer
    {
        return Err("owning-process restart replaced enrolled peer".into());
    }
    println!("BOOTSTRAP_CASE owning_process_restart_keeps_enrolled_peer passed");
    for (fixture, schema, count, probe) in schemas() {
        client.initialize(fixture, schema, 0).await?;
        if client.snapshot(fixture).await?.1.len() != count {
            return Err("history lost across owning process restart".into());
        }
        if let Some(schema) = probe {
            require_probe(
                client
                    .call(fixture, BootstrapCommand::Probe { schema })
                    .await?,
            )?;
        }
    }
    println!("BOOTSTRAP_CASE owning_process_restart_keeps_all_corpora_and_rule_state passed");
    for (fixture, corruption) in [
        ("checksum", HistoryCorruption::Checksum),
        ("noncanonical", HistoryCorruption::NoncanonicalVersion),
        ("unknown", HistoryCorruption::UnknownVersion),
    ] {
        client
            .initialize(fixture, EngineSchema::Registry, 8)
            .await?;
        client
            .call(
                fixture,
                BootstrapCommand::CorruptHistory {
                    schema: EngineSchema::Registry,
                    corruption,
                },
            )
            .await?;
        let failed = client
            .raw(
                fixture,
                BootstrapCommand::Initialize {
                    schema: EngineSchema::Registry,
                },
                false,
            )
            .await?
            .result;
        if !matches!(failed, Err(StoreError::Corrupt { field, .. }) if field == "schema.migration")
        {
            return Err("corrupted migration history was accepted".into());
        }
        println!("BOOTSTRAP_CASE {fixture}_history_refused passed");
    }
    Ok(())
}

async fn run(settings: &WorkerdSettings, client: &Client) -> ConformanceResult<()> {
    let server = WorkerdProcess::start(settings).await?;
    let result = first_process(client).await;
    let stopped = server.stop().await;
    stopped?;
    let peer = result?;
    let server = WorkerdProcess::start(settings).await?;
    let result = second_process(client, &peer).await;
    let stopped = server.stop().await;
    stopped?;
    result
}
#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let node = args.next().ok_or("Node executable")?.into();
    let miniflare = args.next().ok_or("pinned Miniflare entrypoint")?.into();
    let persistence = args.next().ok_or("fresh persistence")?.into();
    let port = args.next().ok_or("port")?.parse::<u16>()?;
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let client = Client {
        http: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()?,
        url: format!("http://127.0.0.1:{port}/admin/v1/conformance/bootstrap"),
    };
    let jwks = JwksFixtureServer::start().await?;
    let settings = WorkerdSettings {
        relay_peer: fleet_sync_protocol::PeerId::try_from(
            "019f0000-0000-7000-8000-000000005240".to_owned(),
        )?,
        node,
        miniflare,
        persistence,
        port,
        jwks_url: jwks.url.clone(),
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let result = run(&settings, &client).await;
    let fixture = jwks.shutdown().await;
    fixture?;
    result?;
    println!(
        "BOOTSTRAP_RESULT 15 passed; 0 failed; 0 ignored (real DO schema/enrollment fixtures; no normal HTTP routing/admission, copy apply or mailbox delivery claim)"
    );
    Ok(())
}
