//! Central dispatch controls select real prefixes; owner Health supplies native Copy ACKs.
use super::{
    central_pair::{SecondHost, central, work_view},
    fixture::{BoardFixture, accepted, at, identity},
    owned_host::BoardHost,
    takeover_readbacks as readbacks,
};
use board_central_test_support::{
    DispatchCallSelector, DispatchFaultObservation, DispatchKey, ObjectAddress, RuntimeControl,
};
use board_sync_protocol::BoardDeliveryTypes;
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use fleet_sync_protocol::{DeliveryItem, ReplicationPosition};
use serde_json::{Value, json};
use std::ops::AsyncFnOnce;

pub use super::follow_fixture::BOUND;

#[derive(Clone, Copy)]
pub enum EndPrefix {
    Discarded,
    Retained,
    LostReceipt,
}

pub async fn assert_prevented_push_replayed(
    wire: &mut board_central_test_support::MailWireObservation,
    item: fleet_sync_protocol::ItemId,
    cancel: fleet_sync_protocol::ItemId,
    report_seq: fleet_sync_protocol::ReportSeq,
) -> FixtureResult {
    use board_central_test_support::MailWireEvent;
    use fleet_sync_protocol::{
        DeliveryFailureKind, DeliveryRefusal, DeliveryStatus, PushResult, RecipientOutcome,
        ReportResult,
    };
    wire.wait_for(|events| {
        let replayed = events.iter().any(|event| {
            let MailWireEvent::PushSent {frame,item:actual} = event else {return false;};
            *actual == item && events.iter().any(|answer| matches!(answer,
                MailWireEvent::PushAnswered {frame:answered,result:PushResult::Replayed {status:DeliveryStatus::Cancelled,cancel:None}} if answered==frame))
        });
        replayed && events.iter().any(|event| {
            let MailWireEvent::ReportSent {frame,item:actual,..}=event else {return false;};
            *actual==item && events.iter().any(|answer| matches!(answer,MailWireEvent::ReportAnswered {frame:answered,..} if answered==frame))
        })
    }).await?;
    let events = wire.events();
    assert_eq!(
        report_count(wire, item),
        1,
        "promotion must send exactly one preserved outcome Report"
    );
    let (frame, sequence, outcome) = events
        .iter()
        .find_map(|event| match event {
            MailWireEvent::ReportSent {
                frame,
                item: actual,
                sequence,
                outcome,
            } if *actual == item => Some((frame, sequence, outcome)),
            _ => None,
        })
        .ok_or("promotion Report absent")?;
    assert_eq!(*sequence, report_seq);
    assert!(
        matches!(outcome,RecipientOutcome::Prevented {cancel:actual} if *actual==cancel),
        "promotion changed preserved outcome: {outcome:?}"
    );
    let answer = events
        .iter()
        .find_map(|event| match event {
            MailWireEvent::ReportAnswered {
                frame: actual,
                answer,
            } if actual == frame => Some(answer),
            _ => None,
        })
        .ok_or("promotion Report acknowledgement absent")?;
    assert!(
        matches!(
            answer,
            ReportResult::Refused {
                reason: DeliveryRefusal::ProcessingFailed {
                    kind: DeliveryFailureKind::ExecutionFailed
                }
            }
        ),
        "Fleet must refuse the local-only cancel with the exact terminal answer: {answer:?}"
    );
    Ok(())
}

fn report_sends(
    events: &[board_central_test_support::MailWireEvent],
    item: fleet_sync_protocol::ItemId,
) -> usize {
    events.iter().filter(|event| matches!(event,
        board_central_test_support::MailWireEvent::ReportSent { item: actual, .. } if *actual == item
    )).count()
}

pub fn report_count(
    wire: &board_central_test_support::MailWireObservation,
    item: fleet_sync_protocol::ItemId,
) -> usize {
    report_sends(&wire.events(), item)
}

pub async fn assert_single_promoted_report(
    report_wire: &board_central_test_support::MailWireObservation,
    status_wire: &mut board_central_test_support::MailWireObservation,
    item: fleet_sync_protocol::ItemId,
) -> FixtureResult {
    let barrier = status_wire.events().len();
    let quiet = status_wire
        .wait_for(|events| {
            report_count(report_wire, item) != 1
                || events
                    .iter()
                    .skip(barrier)
                    .filter(|event| {
                        matches!(
                            event,
                            board_central_test_support::MailWireEvent::StatusFetched(_)
                        )
                    })
                    .count()
                    >= 3
        })
        .await;
    let events = status_wire.events();
    let fetched = events
        .iter()
        .skip(barrier)
        .filter(|event| {
            matches!(
                event,
                board_central_test_support::MailWireEvent::StatusFetched(_)
            )
        })
        .count();
    quiet.map_err(|error| format!("post-Push replay status barrier failed: item{item:?}, priorEvents{barrier}, laterStatusFetches{fetched}, reportSends{}: {error}", report_count(report_wire, item)))?;
    assert_eq!(
        report_count(report_wire, item),
        1,
        "replay or reopen duplicated the promotion Report"
    );
    Ok(())
}

pub async fn assert_end_cut(
    fixture: &BoardFixture,
    stopped: &super::owned_host::StoppedHost,
    original: fleet_sync_protocol::ItemId,
    end: message_board::FeedCursor,
    prefix: EndPrefix,
    expansions: &[Value],
    pre_end_ack: ReplicationPosition,
) -> FixtureResult {
    let original = readbacks::handoff_capture(stopped, &fixture.project, original).await?;
    let end_capture = match prefix {
        EndPrefix::Discarded => {
            readbacks::optional_cause_capture(stopped, &fixture.project, end).await?
        }
        EndPrefix::Retained | EndPrefix::LostReceipt => {
            Some(readbacks::cause_capture(stopped, &fixture.project, end).await?)
        }
    };
    let child = match prefix {
        EndPrefix::Discarded => None,
        EndPrefix::Retained | EndPrefix::LostReceipt => {
            let [expansion] = expansions else {
                return Err("retained end needs one literal child capture".into());
            };
            let child = serde_json::from_value(at(expansion, "/cancel")?.clone())?;
            Some(readbacks::expansion_capture(stopped, &fixture.project, child).await?)
        }
    };
    let ack = match child {
        Some(child) => confirm_copy_ack(fixture, child).await?,
        None => copy_ack(fixture).await?,
    };
    eprintln!(
        "T native cancel prefix evidence: {}",
        json!({"original":original,"end":end_capture,"endCursor":end,"child":child,"preEndAck":pre_end_ack,"copyAck":ack})
    );
    assert_eq!(ack.epoch, original.epoch);
    assert!(ack.sequence >= original.sequence);
    assert_eq!(pre_end_ack.epoch, original.epoch);
    assert!(pre_end_ack.sequence >= original.sequence);
    match (prefix, end_capture) {
        (EndPrefix::Discarded, Some(end)) => assert!(ack.sequence < end.sequence),
        (EndPrefix::Discarded, None) => assert_eq!(ack, pre_end_ack),
        (EndPrefix::Retained | EndPrefix::LostReceipt, Some(end)) => {
            assert!(ack.sequence >= end.sequence)
        }
        (_, None) => return Err("retained end requires an actual native capture".into()),
    }
    Ok(())
}

pub fn copy(fixture: &BoardFixture) -> FixtureResult<ObjectAddress> {
    Ok(ObjectAddress::Copy(fixture.project.clone().try_into()?))
}

pub fn copy_write_key(fixture: &BoardFixture) -> FixtureResult<DispatchKey> {
    Ok(DispatchKey::new(
        copy(fixture)?,
        DispatchCallSelector::CopyWrite,
    ))
}
pub fn submit_key() -> DispatchKey {
    DispatchKey::new(ObjectAddress::Fleet, DispatchCallSelector::FleetMailSubmit)
}
pub fn peer_lookup_key() -> DispatchKey {
    DispatchKey::new(ObjectAddress::Fleet, DispatchCallSelector::InstallationPeer)
}

pub async fn held(
    fixture: &BoardFixture,
    source: ObjectAddress,
    key: DispatchKey,
) -> FixtureResult<Value> {
    let observed = central(fixture)?
        .runtime_control(
            source,
            RuntimeControl::AwaitHeldDispatch { key: key.clone() },
        )
        .await?;
    let DispatchFaultObservation::Held {
        destination,
        request,
    } = observed.dispatch(&key)
    else {
        return Err("required keyed dispatch did not reach its hold".into());
    };
    assert_eq!(destination, key.destination);
    Ok(serde_json::from_slice(&request)?)
}

pub async fn hold_selected(
    fixture: &BoardFixture,
    source: ObjectAddress,
    destination: ObjectAddress,
    call: DispatchCallSelector,
) -> FixtureResult {
    let key = DispatchKey::new(destination.clone(), call);
    let observed = central(fixture)?
        .runtime_control(
            source,
            RuntimeControl::HoldNextDispatch { key: key.clone() },
        )
        .await?;
    assert!(
        matches!(observed.dispatch(&key), DispatchFaultObservation::Armed { destination: actual }
        if actual == destination)
    );
    Ok(())
}

pub async fn held_call(
    fixture: &BoardFixture,
    source: ObjectAddress,
    key: DispatchKey,
    kind: &'static str,
) -> FixtureResult<Value> {
    let request = held(fixture, source, key).await?;
    assert_eq!(request.get("kind"), Some(&json!(kind)));
    Ok(request)
}

pub async fn resume(
    fixture: &BoardFixture,
    source: ObjectAddress,
    key: DispatchKey,
) -> FixtureResult {
    central(fixture)?
        .runtime_control(source, RuntimeControl::ResumeDispatch { key })
        .await?;
    Ok(())
}

pub async fn release_and_drop_selected_reply(
    fixture: &BoardFixture,
    source: ObjectAddress,
    destination: ObjectAddress,
    call: DispatchCallSelector,
) -> FixtureResult<Value> {
    let key = DispatchKey::new(destination, call);
    let request = held(fixture, source.clone(), key.clone()).await?;
    central(fixture)?
        .runtime_control(
            source.clone(),
            RuntimeControl::DropHeldDispatchReply { key: key.clone() },
        )
        .await?;
    resume(fixture, source.clone(), key).await?;
    let dropped = match central(fixture)?
        .runtime_control(source.clone(), RuntimeControl::AwaitDroppedObjectReply)
        .await
    {
        Ok(dropped) => dropped,
        Err(error) => {
            let observed = central(fixture)?
                .runtime_control(source, RuntimeControl::Observe)
                .await;
            return Err(format!(
                "selected reply did not finish: {error}; final control: {observed:?}"
            )
            .into());
        }
    };
    let dropped = serde_json::to_value(dropped.object_reply)?;
    assert_eq!(dropped.get("kind"), Some(&json!("dropped")));
    let dropped: Vec<u8> = serde_json::from_value(at(&dropped, "/value/request")?.clone())?;
    assert_eq!(serde_json::from_slice::<Value>(&dropped)?, request);
    Ok(request)
}

async fn release_if_held(fixture: &BoardFixture, source: ObjectAddress) -> FixtureResult {
    central(fixture)?.release_held_dispatches(source).await
}

pub async fn advance_hold(
    fixture: &BoardFixture,
    source: ObjectAddress,
    key: DispatchKey,
    next: DispatchKey,
) -> FixtureResult {
    central(fixture)?
        .runtime_control(
            source,
            RuntimeControl::ResumeDispatchAndHoldNext { key, next },
        )
        .await?;
    Ok(())
}

pub async fn confirm_prefix(
    fixture: &BoardFixture,
    host: &BoardHost,
    cursor: &Value,
) -> FixtureResult<Value> {
    tokio::time::timeout(BOUND, async {
        loop {
            let view = work_view(host, &fixture.project).await?;
            let actual: message_board::FeedCursor =
                serde_json::from_value(at(&view, "/replicatedThrough")?.clone())?;
            let expected: message_board::FeedCursor = serde_json::from_value(cursor.clone())?;
            if actual.epoch == expected.epoch && actual.position >= expected.position {
                return Ok(view);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn copy_ack(
    fixture: &BoardFixture,
) -> FixtureResult<fleet_sync_protocol::ReplicationPosition> {
    let status = central(fixture)?
        .copy_status(&fixture.project.clone().try_into()?)
        .await?;
    Ok(status.acked)
}

/// Replicate a known immediate local item without advancing its followed thread.
pub async fn replicate_local_handoff(
    fixture: &mut BoardFixture,
    expected: &DeliveryItem<BoardDeliveryTypes>,
) -> FixtureResult<ReplicationPosition> {
    let marker = identity();
    let request = identity();
    let actor = fixture.owner.to_string();
    let record = fixture
        .cli_write(&[
            "workstream",
            "create",
            "--project-id",
            &fixture.project,
            "--workstream-id",
            &marker,
            "--name",
            "native capture barrier",
            "--goal",
            "acknowledge the preceding frozen local handoff",
            "--request-id",
            &request,
            "--actor",
            &actor,
        ])
        .await?;
    accepted(&record)?;
    // The item was read from the joined owner before this unrelated commit.
    // ACK of this later real feed entry therefore includes its earlier WAL.
    let feed = fixture.feed().await?;
    confirm_prefix(fixture, fixture.host()?, at(&feed, "/value/highWater")?).await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("local source Host absent")?
        .stop_for_readback()
        .await?;
    let actual = super::follow_fixture::only_item(readbacks::deliveries(
        readbacks::handoffs(&stopped, &fixture.project, 1).await?,
    ))?;
    assert_eq!(
        serde_json::to_value(&actual)?,
        serde_json::to_value(expected)?
    );
    let captured = readbacks::handoff_capture(&stopped, &fixture.project, actual.id()).await?;
    drop(stopped);
    fixture.reopen_host().await?;
    Ok(captured)
}

pub async fn confirm_copy_ack(
    fixture: &BoardFixture,
    minimum: fleet_sync_protocol::ReplicationPosition,
) -> FixtureResult<fleet_sync_protocol::ReplicationPosition> {
    tokio::time::timeout(BOUND, async {
        loop {
            let actual = copy_ack(fixture).await?;
            if actual.epoch == minimum.epoch && actual.sequence >= minimum.sequence {
                return Ok(actual);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn scenario<F>(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    case: F,
) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture, &'a mut SecondHost) -> FixtureResult,
{
    run_scenario(
        (fixture, second),
        async move |pair: &mut (&mut BoardFixture, &mut SecondHost)| case(pair.0, pair.1).await,
        async |(fixture, second): (&mut BoardFixture, &mut SecondHost)| {
            // Let blocked calls finish before graceful shutdown, then check
            // again after every caller is joined. Keep all independent errors.
            let before_fleet = release_if_held(fixture, ObjectAddress::Fleet).await;
            let first = match fixture.host.take() {
                Some(host) => host.stop().await,
                None => Ok(()),
            };
            let second = second.stop_host().await;
            let fleet = release_if_held(fixture, ObjectAddress::Fleet).await;
            let failures = [
                ("initial Fleet release", before_fleet),
                ("first Host", first),
                ("second Host", second),
                ("Fleet release", fleet),
            ]
            .into_iter()
            .filter_map(|(owner, result)| result.err().map(|error| format!("{owner}: {error}")))
            .collect::<Vec<_>>();
            if failures.is_empty() {
                Ok(())
            } else {
                Err(failures.join("; ").into())
            }
        },
        || Ok(()),
    )
    .await
}
