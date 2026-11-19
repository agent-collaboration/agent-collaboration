//! V9 on workerd: 1,000 due probe instances of one kind complete in four capped
//! due batches 30 s apart through real alarm callbacks, earliest first and each
//! once, with at most eight units in flight. Timings print as measurement only.
use super::super::{Statement, support::*};
use super::engine_event_support::*;
use super::engine_probe_support::*;
use central_storage::SqlValue;
use collaboration_central_core::PausePoint;

const INSTANCES: u32 = 1_000;
const CAP: usize = 250;

/// Instance `index` is due 40 ms after the one before it, from 10:04:10, so all
/// are due by the first batch at 10:05:00 and the earliest-first order is known.
fn due_of(start: Time, index: u32) -> Time {
    at(start, 4, 10, i64::from(index) * 40)
}
async fn fired_items(runtime: &Runtime, group: uuid::Uuid) -> Vec<SqlValue> {
    runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT item_id FROM engine_probe_items WHERE group_id=? AND state='fired' ORDER BY due_at,item_id",
            args: vec![text(group)],
        })
        .await
        .unwrap()
        .rows
        .into_iter()
        .map(|mut row| row.remove(0))
        .collect()
}

#[tokio::test]
async fn a_thousand_due_instances_complete_in_four_capped_batches_thirty_seconds_apart() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let group = probe_id(40000);
    let items: Vec<_> = (0..INSTANCES)
        .map(|index| probe_id(41000 + index))
        .collect();
    for (index, item) in (0..INSTANCES).zip(&items) {
        seed_probe(
            &runtime,
            *item,
            group,
            Some(due_of(start, index)),
            &format!("bulk{index}"),
        )
        .await;
    }
    let expected: Vec<_> = items.iter().map(text).collect();
    let before = callbacks(&runtime).await;
    let mut measurements = Vec::new();

    for (batch, (minutes, seconds)) in [(5, 0), (5, 30), (6, 0), (6, 30)].into_iter().enumerate() {
        let time = at(start, minutes, seconds, 0);
        let started = std::time::Instant::now();
        if batch == 0 {
            // Seeding publishes nothing, so the first batch is an armed callback.
            due_batch_at(&fixture, &runtime, time).await;
        } else {
            assert_eq!(
                alarm_ms(&runtime).await,
                ms(time),
                "batch {batch} is the continuation 30 s after the last start"
            );
            set_now(&fixture, time - chrono::Duration::milliseconds(1));
            assert!(runtime.dispatch_due_alarm().await.unwrap().is_none());
            set_now(&fixture, time);
            assert!(runtime.dispatch_due_alarm().await.unwrap().is_some());
        }
        measurements.push(started.elapsed());
        assert_eq!(
            fired_items(&runtime, group).await,
            expected[..CAP * (batch + 1)],
            "batch {batch} handled exactly the next 250, earliest first"
        );
    }

    assert_eq!(callbacks(&runtime).await, before + 4, "four callbacks");
    assert_eq!(
        alarm_ms(&runtime).await,
        None,
        "nothing is pending afterwards"
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT effect_count FROM engine_probe_groups WHERE group_id=?",
            vec![text(group)]
        )
        .await,
        i64::from(INSTANCES),
        "each instance's effect ran once"
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM engine_work_attempts WHERE machine_kind='engine_probe'",
            vec![]
        )
        .await,
        0
    );
    for (batch, elapsed) in measurements.iter().enumerate() {
        println!(
            "V9 measurement workerd batch {batch}: {CAP} units in {:.3} ms, {:.3} ms per unit (includes the test transport)",
            elapsed.as_secs_f64() * 1e3,
            elapsed.as_secs_f64() * 1e3 / CAP as f64
        );
    }
    fixture.host.close().await;
}

#[tokio::test]
async fn eight_held_units_fill_the_fan_out_and_no_ninth_starts() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let group = probe_id(40001);
    let items: Vec<_> = (0..20).map(|index| probe_id(42000 + index)).collect();
    for (index, item) in (0..20).zip(&items) {
        seed_probe(
            &runtime,
            *item,
            group,
            Some(due_of(start, index)),
            &format!("wide{index}"),
        )
        .await;
    }
    let mut holds = Vec::new();
    for item in &items[..8] {
        holds.push(
            runtime
                .pause_points()
                .hold_engine(PausePoint::AfterEngineLoad, &item.to_string())
                .await,
        );
    }
    let before = runtime.legacy_unit_invocations().await;
    set_now(&fixture, at(start, 5, 0, 0));
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    for held in &mut holds {
        held.until_held().await;
    }
    // Each read is another event that lets a ninth unit run if one had started.
    for _ in 0..16 {
        assert!(
            fired_items(&runtime, group).await.is_empty(),
            "a ninth unit ran while eight were in flight"
        );
    }
    assert_eq!(
        runtime.legacy_unit_invocations().await,
        before,
        "the notice units queue behind the eight as well"
    );

    for held in holds {
        held.release().await;
    }
    batch.await.unwrap().unwrap();

    assert_eq!(fired_items(&runtime, group).await.len(), 20);
    fixture.host.close().await;
}
