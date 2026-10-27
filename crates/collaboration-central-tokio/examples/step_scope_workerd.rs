//! Real-workerd product proof for central's first checked transaction rule.
#![forbid(unsafe_code)]

#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
mod workerd_process;

use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, owner_assertion, receipt_fixture,
    receipt_with_effects_fixture,
};
use collaboration_central_core::{FleetDecisionFault, StepControl, StepControlReply};
use durable_steps::StoreError;
use fleet_admission::FleetRequestResult;
use message_board::{
    CanonicalHash, ChangeOutcome, DecisionEffectsHeader, DecisionEffectsPage, RequestDecision,
    RequestId, RequestLedgerEntry, StoredDecision,
};
use workerd_process::{WorkerdProcess, WorkerdSettings};

struct StepClient {
    http: reqwest::Client,
    url: String,
}
impl StepClient {
    async fn execute(
        &self,
        command: StepControl,
    ) -> ConformanceResult<Result<StepControlReply, StoreError>> {
        Ok(self
            .http
            .post(&self.url)
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&command)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    async fn record(
        &self,
        entry: RequestLedgerEntry,
        fault: FleetDecisionFault,
    ) -> ConformanceResult<Result<StepControlReply, StoreError>> {
        self.record_effects(entry, Vec::new(), fault).await
    }
    async fn record_effects(
        &self,
        entry: RequestLedgerEntry,
        effects: Vec<message_board::DecisionEffect>,
        fault: FleetDecisionFault,
    ) -> ConformanceResult<Result<StepControlReply, StoreError>> {
        self.execute(StepControl::Record {
            entry: Box::new(entry),
            effects,
            fault,
        })
        .await
    }
    async fn read(&self, request_id: RequestId) -> ConformanceResult<StepControlReply> {
        Ok(self.execute(StepControl::Read { request_id }).await??)
    }
}
fn accepted() -> ConformanceResult<RequestDecision> {
    Ok(RequestDecision::Accepted {
        effects: DecisionEffectsPage::from_page(
            Vec::new(),
            &DecisionEffectsHeader {
                count: 0,
                digest: CanonicalHash::try_from(
                    "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945".to_owned(),
                )?,
            },
            None,
        )?,
        change: ChangeOutcome::Changed,
    })
}
fn require(actual: StepControlReply, expected: StepControlReply) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("unexpected step reply: {actual:?}; expected {expected:?}").into());
    }
    Ok(())
}

struct RecordedCases {
    empty: RequestLedgerEntry,
    nonempty: RequestLedgerEntry,
    nonempty_answer: StepControlReply,
}

async fn invalid_first_decisions(client: &StepClient) -> ConformanceResult<()> {
    for (suffix, case) in [
        ("1c1", "empty_rejection"),
        ("1c2", "post_effect"),
        ("1c3", "envelope_id"),
    ] {
        let valid = receipt_fixture(RequestId::try_from(format!(
            "019f0000-0000-7000-8000-000000000{suffix}"
        ))?)?;
        let mut invalid = valid.clone();
        let effects = match case {
            "empty_rejection" => {
                invalid.decision = StoredDecision::Rejected {
                    reasons: Vec::new(),
                };
                Vec::new()
            }
            "post_effect" => {
                let effects = vec![(
                    message_board::PostId::try_from(
                        "019f0000-0000-7000-8000-0000000001cf".to_owned(),
                    )?
                    .into(),
                    invalid.envelope.position.clone(),
                )];
                invalid.decision =
                    StoredDecision::accepted(effects.clone(), ChangeOutcome::Changed);
                let StoredDecision::Accepted {
                    effects: header, ..
                } = &invalid.decision
                else {
                    return Err("matching header is required".into());
                };
                if !header.matches_full(&effects) {
                    return Err("wrong test digest".into());
                }
                effects
            }
            "envelope_id" => {
                invalid.envelope.id = message_board::RecordId::try_from(
                    "019f0000-0000-7000-8000-0000000001ce".to_owned(),
                )?;
                Vec::new()
            }
            _ => return Err("unknown invalid-decision case".into()),
        };
        let result = client
            .record_effects(invalid, effects, FleetDecisionFault::None)
            .await?;
        if !matches!(result, Err(StoreError::Corrupt { .. })) {
            return Err(format!("invalid first {case} was accepted: {result:?}").into());
        }
        require(
            client.read(valid.request_id.clone()).await?,
            StepControlReply::Read(None),
        )?;
        require(
            client
                .record(valid.clone(), FleetDecisionFault::None)
                .await??,
            StepControlReply::Recorded(vec![FleetRequestResult::Decided(accepted()?)]),
        )?;
        require(
            client.read(valid.request_id.clone()).await?,
            StepControlReply::Read(Some(Box::new(valid))),
        )?;
        println!(
            "STEP_CASE invalid_first_{case}_refused_without_writes_then_valid_same_identity passed"
        );
    }
    Ok(())
}
async fn cases(client: &StepClient) -> ConformanceResult<RecordedCases> {
    require(
        client.execute(StepControl::Initialize).await??,
        StepControlReply::Initialized,
    )?;
    invalid_first_decisions(client).await?;
    let first = receipt_fixture(RequestId::try_from(
        "019f0000-0000-7000-8000-000000000101".to_owned(),
    )?)?;
    require(
        client
            .record(first.clone(), FleetDecisionFault::None)
            .await??,
        StepControlReply::Recorded(vec![FleetRequestResult::Decided(accepted()?)]),
    )?;
    require(
        client.read(first.request_id.clone()).await?,
        StepControlReply::Read(Some(Box::new(first.clone()))),
    )?;
    println!("STEP_CASE commit_and_durable_read passed");

    let (with_effects, pairs) = receipt_with_effects_fixture(RequestId::try_from(
        "019f0000-0000-7000-8000-0000000001a1".to_owned(),
    )?)?;
    let recorded = client
        .record_effects(
            with_effects.clone(),
            pairs.clone(),
            FleetDecisionFault::None,
        )
        .await??;
    let StepControlReply::Recorded(answers) = &recorded else {
        return Err("nonempty receipt answer missing".into());
    };
    let [FleetRequestResult::Decided(RequestDecision::Accepted { effects, .. })] =
        answers.as_slice()
    else {
        return Err("nonempty receipt did not answer Accepted".into());
    };
    if effects.entries() != pairs.as_slice() || effects.total_count() != 2 {
        return Err("nonempty receipt changed its ordered effects".into());
    }
    let mut changed = with_effects.clone();
    changed.decision = StoredDecision::Rejected {
        reasons: Vec::new(),
    };
    require(
        client.record(changed, FleetDecisionFault::None).await??,
        recorded.clone(),
    )?;
    require(
        client.read(with_effects.request_id.clone()).await?,
        StepControlReply::Read(Some(Box::new(with_effects.clone()))),
    )?;
    println!("STEP_CASE nonempty_receipts_replay_original_effects passed");

    let mut replay = first.clone();
    replay.decision = StoredDecision::Rejected {
        reasons: Vec::new(),
    };
    require(
        client
            .record(replay.clone(), FleetDecisionFault::None)
            .await??,
        StepControlReply::Recorded(vec![FleetRequestResult::Decided(accepted()?)]),
    )?;
    replay.canonical_hash = CanonicalHash::try_from("b".repeat(64))?;
    require(
        client.record(replay, FleetDecisionFault::None).await??,
        StepControlReply::Recorded(vec![FleetRequestResult::ConflictingRequest]),
    )?;
    require(
        client.read(first.request_id.clone()).await?,
        StepControlReply::Read(Some(Box::new(first.clone()))),
    )?;
    println!("STEP_CASE first_decision_and_conflict passed");

    for (request, fault) in [
        (
            "019f0000-0000-7000-8000-000000000102",
            FleetDecisionFault::CallbackAfterWrite,
        ),
        (
            "019f0000-0000-7000-8000-000000000103",
            FleetDecisionFault::SqlAfterWrite,
        ),
    ] {
        let (entry, pairs) =
            receipt_with_effects_fixture(RequestId::try_from(request.to_owned())?)?;
        let failure = client
            .record_effects(entry.clone(), pairs.clone(), fault)
            .await?;
        let intended = matches!(
            (&fault, &failure),
            (
                FleetDecisionFault::CallbackAfterWrite,
                Err(StoreError::Aborted { .. })
            ) | (
                FleetDecisionFault::SqlAfterWrite,
                Err(StoreError::Backend { .. })
            )
        );
        if !intended {
            return Err(format!("fault did not reach intended store error: {failure:?}").into());
        }
        require(
            client.read(entry.request_id.clone()).await?,
            StepControlReply::Read(None),
        )?;
        // Reusing the exact request after rollback must write both receipts.
        // Orphaned effect rows would also make this transaction fail or decode
        // into a different ordered page instead of these two literal effects.
        let recorded = client
            .record_effects(entry.clone(), pairs.clone(), FleetDecisionFault::None)
            .await??;
        let StepControlReply::Recorded(answers) = &recorded else {
            return Err("post-rollback receipt answer missing".into());
        };
        let [FleetRequestResult::Decided(RequestDecision::Accepted { effects, .. })] =
            answers.as_slice()
        else {
            return Err("post-rollback transaction did not answer Accepted".into());
        };
        if effects.entries() != pairs.as_slice() || effects.total_count() != 2 {
            return Err("post-rollback transaction changed the two ordered receipts".into());
        }
        require(
            client
                .record_effects(entry.clone(), pairs, FleetDecisionFault::None)
                .await??,
            recorded,
        )?;
        require(
            client.read(entry.request_id.clone()).await?,
            StepControlReply::Read(Some(Box::new(entry))),
        )?;
        println!("STEP_CASE rollback_{fault:?}_then_same_identity passed");
    }
    Ok(RecordedCases {
        empty: first,
        nonempty: with_effects,
        nonempty_answer: recorded,
    })
}

#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut arguments = std::env::args().skip(1);
    let node = arguments.next().ok_or("Node executable")?.into();
    let miniflare = arguments
        .next()
        .ok_or("pinned Miniflare entrypoint")?
        .into();
    let persistence = arguments
        .next()
        .ok_or("fresh persistence directory")?
        .into();
    let port = arguments.next().ok_or("loopback port")?.parse::<u16>()?;
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
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
    let client = StepClient {
        http: reqwest::Client::builder()
            .http1_only()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()?,
        url: format!("http://127.0.0.1:{port}/admin/v1/conformance/fleet-receipt"),
    };
    let server = match WorkerdProcess::start(&settings).await {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let result = cases(&client).await;
    let stopped = server.stop().await;
    let recorded = match result {
        Ok(recorded) => recorded,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    if let Err(error) = stopped {
        jwks.shutdown().await?;
        return Err(error);
    }
    let server = match WorkerdProcess::start(&settings).await {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let reopened = async {
        let first = recorded.empty;
        require(
            client.read(first.request_id.clone()).await?,
            StepControlReply::Read(Some(Box::new(first.clone()))),
        )?;
        let mut replay = first;
        replay.decision = StoredDecision::Rejected {
            reasons: Vec::new(),
        };
        require(
            client.record(replay, FleetDecisionFault::None).await??,
            StepControlReply::Recorded(vec![FleetRequestResult::Decided(accepted()?)]),
        )?;
        println!("STEP_CASE real_workerd_restart_preserves_receipt passed");
        require(
            client.read(recorded.nonempty.request_id.clone()).await?,
            StepControlReply::Read(Some(Box::new(recorded.nonempty.clone()))),
        )?;
        let mut replay = recorded.nonempty;
        replay.decision = StoredDecision::Rejected {
            reasons: Vec::new(),
        };
        require(
            client.record(replay, FleetDecisionFault::None).await??,
            recorded.nonempty_answer,
        )?;
        println!("STEP_CASE real_workerd_restart_preserves_nonempty_effects passed");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = server.stop().await;
    let fixture = jwks.shutdown().await;
    reopened?;
    stopped?;
    fixture?;
    println!(
        "STEP_RESULT 10 distinct groups passed; 0 failed; 0 ignored (real DO store and SQLx; test HTTP controls, no production route claim)"
    );
    Ok(())
}
