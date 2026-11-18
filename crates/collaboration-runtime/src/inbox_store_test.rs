use super::{InboxStore, RecordDelivery};
use crate::MachineLocalStore;
use hegel::{TestCase, generators as gs};
use router_clock::ManualClock;
use runtime_primitives::{Clock, SyncTime};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use workspace_model::{
    MessageId,
    central_mail::{DeliveryAnswer, MessageRecord, MessageRefusal},
};

fn message(index: u8) -> MessageRecord {
    serde_json::from_value(serde_json::json!({
        "messageId": format!("01890f2e-7b4c-7cc0-98c4-{index:012x}"),
        "sender": { "kind": "central" },
        "target": {
            "machineId": "01890f2e-7b4c-7cc0-98c4-000000000002",
            "endpoint": "codex-local",
            "sessionId": "receiver",
        },
        "body": { "kind": "direct", "body": "hello", "delivery": "auto", "replyTo": null },
        "state": { "kind": "queued" },
        "source": null,
        "createdAt": "2030-01-01T00:00:00Z",
        "updatedAt": "2030-01-01T00:00:00Z",
    }))
    .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Event {
    Deliver,
    Repeat,
    LoseAck,
    Flush,
    Reconnect,
}

struct InboxTrial {
    store: InboxStore,
    clock: ManualClock,
    connected: bool,
    delivered: BTreeSet<MessageId>,
    handoffs: BTreeMap<MessageId, usize>,
    settled: BTreeMap<MessageId, DeliveryAnswer>,
}

impl InboxTrial {
    async fn deliver(&mut self, index: u8) {
        assert!(self.connected);
        let message = message(index);
        let now = self.clock.now();
        let mark = central_link_client::next_mark_after(now);
        let first_push = self.delivered.insert(message.message_id.clone());
        let recorded = self
            .store
            .record_delivery(&message, mark, now)
            .await
            .unwrap();
        assert_eq!(matches!(recorded, RecordDelivery::Inserted), first_push);
        if let RecordDelivery::Inserted = recorded {
            let handoff = self.handoffs.entry(message.message_id.clone()).or_default();
            *handoff += 1;
            let answer = answer(index);
            self.store
                .record_handoff(&message.message_id, answer, mark, now)
                .await
                .unwrap();
        }
    }

    async fn flush(&mut self, disconnect_before_receipt: bool) {
        self.clock.advance(Duration::from_secs(60)).unwrap();
        let now = self.clock.now();
        if !self.connected {
            return;
        }
        for ack in self.store.acks_due(now).await.unwrap() {
            self.store
                .mark_ack_sent(&ack.message_id, now)
                .await
                .unwrap();
            if disconnect_before_receipt {
                // The socket write succeeds locally, but central sees no Ack.
                self.connected = false;
                break;
            }
            if let Some(previous) = self.settled.insert(ack.message_id, ack.answer) {
                assert_eq!(previous, ack.answer, "a repeat changes the central answer");
            }
        }
    }

    async fn reconnect(&mut self) {
        self.connected = true;
        for index in 0..4 {
            let id = message(index).message_id;
            if self.delivered.contains(&id) && !self.settled.contains_key(&id) {
                self.deliver(index).await;
            }
        }
    }
}

fn answer(index: u8) -> DeliveryAnswer {
    if index.is_multiple_of(2) {
        DeliveryAnswer::Accepted
    } else {
        DeliveryAnswer::Refused {
            reason: MessageRefusal::RecipientRefused,
        }
    }
}

#[hegel::test(test_cases = 48)]
fn ambiguous_ack_replays_reach_one_settlement_without_repeating_handoff(tc: TestCase) {
    let history = tc.draw(gs::vecs(
        gs::tuples!(
            gs::sampled_from(vec![
                Event::Deliver,
                Event::Repeat,
                Event::LoseAck,
                Event::Flush,
                Event::Reconnect,
            ]),
            gs::integers::<u8>().max_value(3)
        )
        .max_size(20),
    ));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("machine_local.sqlite");
        let mut trial = InboxTrial {
            store: InboxStore::new(MachineLocalStore::open(&path).await.unwrap()),
            clock: ManualClock::starting_at(SyncTime::from_unix_millis(0)).unwrap(),
            connected: true,
            delivered: BTreeSet::new(),
            handoffs: BTreeMap::new(),
            settled: BTreeMap::new(),
        };
        trial.deliver(0).await;
        assert!(
            trial
                .store
                .acks_due(trial.clock.now())
                .await
                .unwrap()
                .is_empty(),
            "an Ack must wait until its minute mark"
        );
        trial.flush(true).await;
        assert!(trial.settled.is_empty());
        // Read the deduplication state through a freshly opened SQLite handle.
        trial.store = InboxStore::new(MachineLocalStore::open(&path).await.unwrap());
        trial.reconnect().await;
        trial.flush(false).await;
        assert_eq!(trial.settled.get(&message(0).message_id), Some(&answer(0)));
        for (event, index) in history {
            match event {
                Event::Deliver if trial.connected => trial.deliver(index).await,
                Event::Repeat
                    if trial.connected && trial.delivered.contains(&message(index).message_id) =>
                {
                    trial.deliver(index).await;
                }
                Event::LoseAck => trial.flush(true).await,
                Event::Flush => trial.flush(false).await,
                Event::Reconnect => trial.reconnect().await,
                Event::Deliver | Event::Repeat => {}
            }
        }
        trial.reconnect().await;
        trial.flush(false).await;
        assert!(
            trial
                .store
                .acks_due(trial.clock.now())
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(trial.settled.len(), trial.delivered.len());
        for index in 0..4 {
            let id = message(index).message_id;
            if trial.delivered.contains(&id) {
                assert_eq!(trial.handoffs.get(&id), Some(&1));
                assert_eq!(trial.settled.get(&id), Some(&answer(index)));
            }
        }
        let mut database = MachineLocalStore::open(&path).await.unwrap();
        let (rows, retained_payloads): (i64, i64) =
            sqlx::query_as("SELECT count(*), count(message_json) FROM inbox_messages")
                .fetch_one(&mut database.connection)
                .await
                .unwrap();
        assert_eq!(usize::try_from(rows).unwrap(), trial.delivered.len());
        assert_eq!(retained_payloads, 0);
    });
}

#[tokio::test]
async fn inbox_prunes_only_sent_answers_older_than_retention() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("machine_local.sqlite");
    let clock = ManualClock::starting_at(SyncTime::from_unix_millis(0)).unwrap();
    let mut store = InboxStore::new(MachineLocalStore::open(&path).await.unwrap());

    let now = clock.now();
    let mark = central_link_client::next_mark_after(now);
    for index in 0..3 {
        assert_eq!(
            store
                .record_delivery(&message(index), mark, now)
                .await
                .unwrap(),
            RecordDelivery::Inserted
        );
    }
    for index in 0..2 {
        store
            .record_handoff(&message(index).message_id, answer(index), mark, now)
            .await
            .unwrap();
    }
    clock.advance(Duration::from_secs(60)).unwrap();
    store
        .mark_ack_sent(&message(0).message_id, clock.now())
        .await
        .unwrap();

    clock.advance(Duration::from_secs(24 * 60 * 60)).unwrap();
    store.prune_ended(clock.now()).await.unwrap();
    let next_mark = central_link_client::next_mark_after(clock.now());
    assert_eq!(
        store
            .record_delivery(&message(0), next_mark, clock.now())
            .await
            .unwrap(),
        RecordDelivery::Duplicate,
        "the exact 24-hour boundary still retains the answer"
    );
    clock.advance(Duration::from_secs(60)).unwrap();
    store
        .mark_ack_sent(&message(0).message_id, clock.now())
        .await
        .unwrap();
    clock
        .advance(Duration::from_secs(24 * 60 * 60 + 1))
        .unwrap();
    store.prune_ended(clock.now()).await.unwrap();

    let mut database = MachineLocalStore::open(&path).await.unwrap();
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT message_id FROM inbox_messages ORDER BY message_id")
            .fetch_all(&mut database.connection)
            .await
            .unwrap();
    assert_eq!(
        ids,
        vec![
            message(1).message_id.as_str(),
            message(2).message_id.as_str()
        ]
    );
}

#[tokio::test]
async fn inbox_rejects_unknown_state_and_a_payload_with_the_wrong_message_id() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("machine_local.sqlite");
    let now = SyncTime::from_unix_millis(0);
    let mark = central_link_client::next_mark_after(now);
    let first = message(0);
    let mut store = InboxStore::new(MachineLocalStore::open(&path).await.unwrap());
    store.record_delivery(&first, mark, now).await.unwrap();

    let mut database = MachineLocalStore::open(&path).await.unwrap();
    sqlx::query("UPDATE inbox_messages SET delivery_state_json = ?")
        .bind(r#"{"kind":"unknown"}"#)
        .execute(&mut database.connection)
        .await
        .unwrap();
    let result = store.record_delivery(&first, mark, now).await;
    assert!(matches!(
        result,
        Err(super::InboxStoreError::Corrupt {
            field: "delivery_state_json",
            ..
        })
    ));

    sqlx::query("UPDATE inbox_messages SET delivery_state_json = ?, message_json = ?")
        .bind(r#"{"kind":"pendingHandoff"}"#)
        .bind(serde_json::to_string(&message(1)).unwrap())
        .execute(&mut database.connection)
        .await
        .unwrap();
    let result = store.pending_handoffs().await;
    assert!(matches!(
        result,
        Err(super::InboxStoreError::Corrupt {
            field: "message_json.message_id",
            ..
        })
    ));
}
