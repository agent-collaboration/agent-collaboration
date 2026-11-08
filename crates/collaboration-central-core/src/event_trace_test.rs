use super::*;
use crate::ObjectAddress;
use durable_steps::{BoxFuture, CallFailure, Uuid};
use fleet_protocol::ConnectionId;
use runtime_primitives::SyncTime;
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Default)]
struct CollectingObserver {
    records: Mutex<Vec<EventRecord>>,
}

impl KernelObserver for CollectingObserver {
    fn object(&self) -> ObjectAddress {
        ObjectAddress::Fleet
    }
    fn construction(&self) -> Uuid {
        Uuid::nil()
    }
    fn emit(&self, record: EventRecord) {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(record);
    }
    fn barrier(
        &self,
        _point: BarrierPoint,
        _unit_kind: Option<&'static str>,
        _event: Uuid,
    ) -> BoxFuture<'static, ()> {
        Box::pin(std::future::ready(()))
    }
}

fn finish(trace: &EventTrace) {
    trace.finish(EventBounds {
        sequence: 0,
        event: EventKindRecord::Alarm { redelivery: None },
        entered_at: SyncTime::from_unix_millis(1),
        exited_at: SyncTime::from_unix_millis(2),
        alarm_before: None,
        alarm_after: None,
        reply: ReplyRecord::AlarmDone,
    });
}

fn take(observer: &CollectingObserver) -> Vec<EventRecord> {
    std::mem::take(
        &mut *observer
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    )
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_nested_event_keeps_its_own_sends_and_restores_the_outer_scope()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: an outer event whose call is served inline by another object.
    let observer = Arc::new(CollectingObserver::default());
    let outer = Arc::new(EventTrace::new(observer.clone(), EventClass::Alarm));
    let inner = Arc::new(EventTrace::new(observer.clone(), EventClass::Alarm));
    let outer_connection = ConnectionId::new(Uuid::now_v7())?;
    let inner_connection = ConnectionId::new(Uuid::now_v7())?;

    // Act
    scoped(outer.clone(), async {
        record_send(outer_connection, br#"{"kind":"welcome"}"#);
        scoped(inner.clone(), async {
            record_send(inner_connection, b"not json");
        })
        .await;
        record_send(outer_connection, b"after");
    })
    .await;
    record_send(outer_connection, b"outside any event");
    finish(&outer);
    finish(&inner);

    // Assert: each send lands in the event that made it, and none outside one.
    let records = take(&observer);
    let outer_sends: Vec<_> = records[0]
        .sends
        .iter()
        .map(|send| send.frame.clone())
        .collect();
    let inner_sends: Vec<_> = records[1]
        .sends
        .iter()
        .map(|send| send.frame.clone())
        .collect();
    assert_eq!(
        outer_sends,
        [br#"{"kind":"welcome"}"#.to_vec(), b"after".to_vec()]
    );
    assert_eq!(inner_sends, [b"not json".to_vec()]);
    assert_eq!(records[0].sends[0].request_kind.as_deref(), Some("welcome"));
    assert_eq!(records[1].sends[0].request_kind, None);
    Ok(())
}

#[tokio::test]
async fn a_call_the_event_stops_waiting_for_is_recorded_as_deadline_exceeded() {
    // Arrange
    let observer = Arc::new(CollectingObserver::default());
    let trace = Arc::new(EventTrace::new(observer.clone(), EventClass::Alarm));
    trace.step(PipelineStep::LongOperations);

    // Act: one answered call, one failed, one abandoned at the event's deadline.
    scoped(trace.clone(), async {
        let _ = traced_call(&ObjectAddress::Fleet, br#"{"kind":"a"}"#, async {
            Ok(Vec::new())
        })
        .await;
        let _ = traced_call(&ObjectAddress::Fleet, b"{}", async {
            Err(CallFailure::AnswerLost)
        })
        .await;
        let abandoned = traced_call(&ObjectAddress::Fleet, b"{}", std::future::pending());
        let _ = futures_util::future::select(Box::pin(abandoned), std::future::ready(())).await;
    })
    .await;
    finish(&trace);

    // Assert
    let records = take(&observer);
    let outcomes: Vec<_> = records[0].calls.iter().map(|call| call.outcome).collect();
    assert_eq!(
        outcomes,
        [
            CallOutcomeRecord::Answered,
            CallOutcomeRecord::Failed,
            CallOutcomeRecord::DeadlineExceeded
        ]
    );
    assert_eq!(records[0].calls[0].call_kind.as_deref(), Some("a"));
    assert_eq!(
        records[0].calls[0].unit_kind.as_deref(),
        Some("long_operations")
    );
}

#[tokio::test]
async fn alarm_writes_name_the_step_that_made_them() {
    // Arrange
    let observer = Arc::new(CollectingObserver::default());
    let trace = Arc::new(EventTrace::new(observer.clone(), EventClass::Alarm));

    // Act
    scoped(trace.clone(), async {
        trace.step(PipelineStep::Handle);
        record_lower(SyncTime::from_unix_millis(5_000)).await;
        trace.step(PipelineStep::Recompute);
        trace.exact(None);
    })
    .await;
    finish(&trace);

    // Assert
    let records = take(&observer);
    assert_eq!(
        records[0].alarm.writes,
        [
            AlarmWriteRecord {
                op: AlarmWriteOp::Lower,
                at: Some(5_000),
                unit_kind: Some("handler".into())
            },
            AlarmWriteRecord {
                op: AlarmWriteOp::Exact,
                at: None,
                unit_kind: Some("recompute".into())
            }
        ]
    );
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn an_unbound_send_serializes_every_slice_four_field_as_null()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange
    let send = SendRecord {
        attempt: None,
        attempt_kind: None,
        subject: None,
        send_id: None,
        connection: ConnectionId::new(Uuid::from_u128(0x019f_0000_0000_7000_8000_0000_0000_00d1))?,
        session: None,
        request_kind: Some("welcome".into()),
        sent_version: None,
        batch: None,
        frame: b"{}".to_vec(),
    };

    // Act
    let wire = serde_json::to_value(&send)?;

    // Assert: the v3.4 literal; null fields are present, never absent.
    assert_eq!(
        wire,
        serde_json::json!({
            "attempt": null, "attemptKind": null, "subject": null, "sendId": null,
            "connection": "019f0000-0000-7000-8000-0000000000d1", "session": null,
            "requestKind": "welcome", "sentVersion": null, "batch": null, "frame": "e30="
        })
    );
    Ok(())
}
