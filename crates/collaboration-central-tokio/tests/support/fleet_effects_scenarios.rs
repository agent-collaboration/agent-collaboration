//! The real consumer route reads independently seeded immutable receipt rows.

use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardReadPage, BoardReadScope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion, receipt_fixture,
};
use collaboration_central_core::{FleetDecisionFault, StepControl, StepControlReply};
use durable_steps::StoreError;
use fleet_sync_protocol::{ClientFrame, FrameId, OperationAnswer, OperationRequest, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use message_board::{
    BoardId, ChangeOutcome, DecisionEffect, DecisionEffectsPage, FeedPosition, FleetFeedEntry,
    FleetPosition, RecordPlacement, Refusal, RequestDecision, RequestEffectsCursor,
    RequestEffectsRead, RequestEffectsRefusal, RequestId, RequestLedgerEntry, StoredDecision,
    WorkspaceId,
};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;

pub fn request_id() -> ConformanceResult<RequestId> {
    Ok(RequestId::try_from(
        "019f0000-0000-7000-8000-000000000f31".to_owned(),
    )?)
}
fn empty_request() -> ConformanceResult<RequestId> {
    Ok(RequestId::try_from(
        "019f0000-0000-7000-8000-000000000f32".to_owned(),
    )?)
}
fn rejected_request() -> ConformanceResult<RequestId> {
    Ok(RequestId::try_from(
        "019f0000-0000-7000-8000-000000000f33".to_owned(),
    )?)
}
fn fixture() -> ConformanceResult<(RequestLedgerEntry, Vec<DecisionEffect>)> {
    let mut entry = receipt_fixture(request_id()?)?;
    let RecordPlacement::Fleet(placement) = &entry.envelope.position else {
        return Err("fixture is not a Fleet receipt".into());
    };
    let origin = placement.origin.clone();
    let mut effects = Vec::new();
    for ordinal in 0_u64..300 {
        effects.push((
            BoardId::try_from(format!("019f0000-0000-7000-8000-{ordinal:012x}"))?.into(),
            RecordPlacement::Fleet(FleetFeedEntry {
                position: FleetPosition::try_from(ordinal.saturating_add(1))?,
                origin: origin.clone(),
            }),
        ));
    }
    entry.decision = StoredDecision::accepted(effects.clone(), ChangeOutcome::Changed);
    Ok((entry, effects))
}
pub async fn seed(base: &str) -> ConformanceResult<()> {
    let (entry, effects) = fixture()?;
    record(base, entry, effects).await?;
    record(base, receipt_fixture(empty_request()?)?, Vec::new()).await?;
    let mut rejected = receipt_fixture(rejected_request()?)?;
    rejected.decision = StoredDecision::Rejected {
        reasons: vec![Refusal::Archived],
    };
    record(base, rejected, Vec::new()).await
}
async fn record(
    base: &str,
    entry: RequestLedgerEntry,
    effects: Vec<DecisionEffect>,
) -> ConformanceResult<()> {
    let command = StepControl::Record {
        entry: Box::new(entry),
        effects,
        fault: FleetDecisionFault::None,
    };
    // The repeated fixture call exercises the checked ledger's real receipt
    // reconstruction, including the 9/10 numeric ordinal boundary.
    for _ in 0..2 {
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()?
            .post(format!("{base}/admin/v1/conformance/fleet-receipt"))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&command)
            .send()
            .await?
            .error_for_status()?
            .json::<Result<StepControlReply, StoreError>>()
            .await??;
        if !matches!(response, StepControlReply::Recorded(_)) {
            return Err("receipt fixture was not committed by the real checked step".into());
        }
    }
    Ok(())
}
fn scope(
    workspace: WorkspaceId,
    request: RequestId,
    after: Option<RequestEffectsCursor>,
) -> BoardReadScope {
    BoardReadScope::RequestEffects {
        workspace,
        request,
        after,
    }
}
fn accepted(answer: RequestEffectsRead) -> ConformanceResult<DecisionEffectsPage> {
    match answer {
        RequestEffectsRead::Decision {
            decision:
                RequestDecision::Accepted {
                    effects,
                    change: ChangeOutcome::Changed,
                },
        } => Ok(effects),
        _ => Err("receipt did not retain its accepted decision and change outcome".into()),
    }
}
fn refusal(answer: RequestEffectsRead, expected: RequestEffectsRefusal) -> ConformanceResult<()> {
    if answer != (RequestEffectsRead::Refused { reason: expected }) {
        return Err("request effects read did not retain its typed refusal".into());
    }
    Ok(())
}
pub async fn pages_and_refusals(base: &str, reopened: bool) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let mut peer = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-4466554400f2",
        "019f0000-0000-7000-8000-000000001021",
        "fleet-page-reader",
        if reopened {
            "019f0000-0000-7000-8000-000000000f42"
        } else {
            "019f0000-0000-7000-8000-000000000f41"
        },
    )
    .await?;
    let result = async {
        let (entry, expected) = fixture()?;
        let workspace = entry.envelope.scope.workspace;
        let first = accepted(
            read(
                &mut peer.socket,
                scope(workspace.clone(), request_id()?, None),
            )
            .await?,
        )?;
        if first.entries()
            != expected
                .get(..256)
                .ok_or("fixture needs 256 initial effects")?
            || first.total_count() != 300
            || first.next() != Some(&RequestEffectsCursor::Fleet { ordinal: 255 })
        {
            return Err(
                "first receipt page was truncated, mixed, reordered or lost its continuation"
                    .into(),
            );
        }
        let next = accepted(
            read(
                &mut peer.socket,
                scope(workspace.clone(), request_id()?, first.next().cloned()),
            )
            .await?,
        )?;
        if next.entries() != expected.get(256..).ok_or("fixture needs a continuation")?
            || next.entries().len() != 44
            || next.total_count() != 300
            || next.digest() != first.digest()
            || next.next().is_some()
        {
            return Err("continuation repeated, lost or mixed immutable effects".into());
        }
        let exhausted = accepted(
            read(
                &mut peer.socket,
                scope(
                    workspace.clone(),
                    request_id()?,
                    Some(RequestEffectsCursor::Fleet { ordinal: 299 }),
                ),
            )
            .await?,
        )?;
        if !exhausted.entries().is_empty()
            || exhausted.total_count() != 300
            || exhausted.next().is_some()
        {
            return Err("the final valid ordinal did not give an exhausted page".into());
        }
        refusal(
            read(
                &mut peer.socket,
                scope(
                    workspace.clone(),
                    RequestId::try_from("019f0000-0000-7000-8000-000000000f98".to_owned())?,
                    None,
                ),
            )
            .await?,
            RequestEffectsRefusal::UnknownRequest,
        )?;
        refusal(
            read(
                &mut peer.socket,
                scope(
                    WorkspaceId::try_from("019f0000-0000-7000-8000-000000000f99".to_owned())?,
                    request_id()?,
                    None,
                ),
            )
            .await?,
            RequestEffectsRefusal::RequestNotInScope,
        )?;
        for cursor in [
            RequestEffectsCursor::Fleet { ordinal: 300 },
            RequestEffectsCursor::Fleet { ordinal: u64::MAX },
            RequestEffectsCursor::Project {
                position: FeedPosition::try_from(1)?,
            },
        ] {
            refusal(
                read(
                    &mut peer.socket,
                    scope(workspace.clone(), request_id()?, Some(cursor)),
                )
                .await?,
                RequestEffectsRefusal::CursorInvalid,
            )?;
        }
        let empty = accepted(
            read(
                &mut peer.socket,
                scope(workspace.clone(), empty_request()?, None),
            )
            .await?,
        )?;
        if !empty.entries().is_empty() || empty.total_count() != 0 || empty.next().is_some() {
            return Err("empty receipt read leaked another request's rows".into());
        }
        refusal(
            read(
                &mut peer.socket,
                scope(
                    workspace.clone(),
                    empty_request()?,
                    Some(RequestEffectsCursor::Fleet { ordinal: 0 }),
                ),
            )
            .await?,
            RequestEffectsRefusal::CursorInvalid,
        )?;
        if read(
            &mut peer.socket,
            scope(workspace.clone(), rejected_request()?, None),
        )
        .await?
            != (RequestEffectsRead::Decision {
                decision: RequestDecision::Rejected {
                    reasons: vec![Refusal::Archived],
                },
            })
        {
            return Err("rejected receipt did not retain its original decision".into());
        }
        refusal(
            read(
                &mut peer.socket,
                scope(
                    workspace.clone(),
                    rejected_request()?,
                    Some(RequestEffectsCursor::Fleet { ordinal: 0 }),
                ),
            )
            .await?,
            RequestEffectsRefusal::CursorInvalid,
        )?;
        // Refusals leave this very socket usable, without a reconnect.
        let final_read = accepted(
            read(
                &mut peer.socket,
                scope(
                    workspace,
                    request_id()?,
                    Some(RequestEffectsCursor::Fleet { ordinal: 255 }),
                ),
            )
            .await?,
        )?;
        if final_read.entries().len() != 44 {
            return Err("a refused read broke the session".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}
pub async fn read(
    socket: &mut WebSocket,
    scope: BoardReadScope,
) -> ConformanceResult<RequestEffectsRead> {
    let scope_description = format!("{scope:?}");
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Consumer(BoardConsumerRequest::Read {
                    scope,
                    cursor: None,
                }),
            })?
            .into(),
        ))
        .await?;
    let message = tokio::time::timeout(Duration::from_secs(8), socket.next())
        .await?
        .ok_or("effects read lost its session")??;
    let bytes = match message {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        other => {
            return Err(format!(
                "effects read {scope_description} closed instead of answering: {other:?}"
            )
            .into());
        }
    };
    match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
        ServerFrame::Answer {
            frame: actual,
            body:
                OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::RequestEffects {
                    result,
                    ..
                })),
        } if actual == frame => Ok(result),
        _ => Err("effects read lost frame correlation or its consumer answer family".into()),
    }
}
