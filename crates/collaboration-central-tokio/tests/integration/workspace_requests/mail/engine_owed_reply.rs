//! V1: the owed-reply reducer through `handle` on the native host (P10).
use super::super::support::*;
use super::engine_probe_support::*;
use collaboration_central_core::{
    PausePoint,
    engine_owed_probe::{
        EngineReceipt, EngineRefusal, EngineRequestOutcome, OwedAnswer, OwedEvent, OwedFinish,
        OwedReplyRefusal, OwedReplyRequest, OwedState, SeedOwedReply,
    },
};

type OwedOutcome = EngineRequestOutcome<OwedState, OwedReplyRefusal>;

async fn owed(
    runtime: &Runtime,
    request: u32,
    item: uuid::Uuid,
    event: OwedEvent,
    fixture: &Fixture,
) -> OwedOutcome {
    runtime
        .request_owed_reply(
            &fixture.caller,
            OwedReplyRequest {
                request_id: id(request),
                key: key(item),
                event,
            },
        )
        .await
        .unwrap()
}
fn sent(outcome: &OwedOutcome) -> (uuid::Uuid, uuid::Uuid) {
    match outcome {
        EngineRequestOutcome::Answer {
            receipt:
                EngineReceipt::Applied {
                    state:
                        OwedState::InFlight {
                            send_id,
                            connection,
                        },
                },
        } => (*send_id, *connection),
        other => panic!("not sent: {other:?}"),
    }
}
async fn owed_facts(
    runtime: &Runtime,
    item: uuid::Uuid,
    group: uuid::Uuid,
) -> Vec<central_storage::SqlValue> {
    runtime.statement_host().read(super::super::Statement {
        sql: "SELECT state,send_id,(SELECT effect_count FROM engine_probe_groups WHERE group_id=?),completed_claim IS NOT NULL FROM engine_probe_items WHERE item_id=? AND machine_kind='owed_reply'",
        args: vec![text(group), text(item)],
    }).await.unwrap().rows.into_iter().next().unwrap()
}
async fn seed_owed(runtime: &Runtime, item: uuid::Uuid, group: uuid::Uuid) {
    assert!(matches!(
        runtime
            .seed_owed_reply(SeedOwedReply { key: item, group })
            .await
            .unwrap(),
        central_storage::BatchOutcome::Applied { .. }
    ));
}

#[tokio::test]
async fn a_late_answer_to_an_older_send_after_reconnect_changes_nothing() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let (item, group) = (probe_id(9300), probe_id(9301));
    let (first_connection, second_connection) = (probe_id(9302), probe_id(9303));
    seed_owed(&runtime, item, group).await;
    let (old_send, _) = sent(
        &owed(
            &runtime,
            9310,
            item,
            OwedEvent::Send {
                connection: first_connection,
            },
            &fixture,
        )
        .await,
    );
    assert!(matches!(
        owed(
            &runtime,
            9311,
            item,
            OwedEvent::Reconnected {
                connection: second_connection
            },
            &fixture
        )
        .await,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied {
                state: OwedState::Waiting
            }
        }
    ));
    let (new_send, _) = sent(
        &owed(
            &runtime,
            9312,
            item,
            OwedEvent::Send {
                connection: second_connection,
            },
            &fixture,
        )
        .await,
    );
    assert_ne!(old_send, new_send, "every send mints its own id");

    let late = owed(
        &runtime,
        9313,
        item,
        OwedEvent::Answer {
            send_id: old_send,
            connection: first_connection,
            answer: OwedAnswer::Accepted,
        },
        &fixture,
    )
    .await;

    assert_eq!(
        late,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Refused {
                error: EngineRefusal::Stale
            }
        }
    );
    assert_eq!(
        owed_facts(&runtime, item, group).await,
        vec![text("in_flight"), text(new_send), integer(2), integer(0)],
        "the late answer leaves the newer send in flight"
    );
    assert!(matches!(
        owed(
            &runtime,
            9314,
            item,
            OwedEvent::Answer {
                send_id: new_send,
                connection: second_connection,
                answer: OwedAnswer::Accepted,
            },
            &fixture,
        )
        .await,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied {
                state: OwedState::Done {
                    finish: OwedFinish::Delivered
                }
            }
        }
    ));
    fixture.host.close().await;
}

#[tokio::test]
async fn two_answers_racing_on_one_owed_reply_commit_once() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let (item, group, connection) = (probe_id(9320), probe_id(9321), probe_id(9322));
    seed_owed(&runtime, item, group).await;
    let (send_id, _) = sent(
        &owed(
            &runtime,
            9330,
            item,
            OwedEvent::Send { connection },
            &fixture,
        )
        .await,
    );
    let points = runtime.pause_points();
    let mut first_hold = points.hold_engine(PausePoint::AfterEngineLoad, &item.to_string());
    let mut second_hold = points.hold_engine(PausePoint::AfterEngineLoad, &item.to_string());
    let answer = |request, answer| {
        let runtime = runtime.clone();
        let caller = fixture.caller.clone();
        tokio::spawn(async move {
            runtime
                .request_owed_reply(
                    &caller,
                    OwedReplyRequest {
                        request_id: id(request),
                        key: key(item),
                        event: OwedEvent::Answer {
                            send_id,
                            connection,
                            answer,
                        },
                    },
                )
                .await
                .unwrap()
        })
    };
    let accepted = answer(9331, OwedAnswer::Accepted);
    first_hold.until_held().await;
    let refused = answer(9332, OwedAnswer::RefusedForGood);
    second_hold.until_held().await;
    first_hold.release();
    second_hold.release();

    let answers = [accepted.await.unwrap(), refused.await.unwrap()];

    let applied = answers
        .iter()
        .filter(|answer| {
            matches!(
                answer,
                EngineRequestOutcome::Answer {
                    receipt: EngineReceipt::Applied { .. }
                }
            )
        })
        .count();
    let stale = answers
        .iter()
        .filter(|answer| {
            **answer
                == EngineRequestOutcome::Answer {
                    receipt: EngineReceipt::Refused {
                        error: EngineRefusal::Stale,
                    },
                }
        })
        .count();
    assert_eq!((applied, stale), (1, 1), "{answers:?}");
    let facts = owed_facts(&runtime, item, group).await;
    assert!(
        facts[0] == text("delivered") || facts[0] == text("refused"),
        "{facts:?}"
    );
    assert_eq!(
        &facts[2..],
        &[integer(1), integer(1)],
        "one send, one finish"
    );
    fixture.host.close().await;
}
