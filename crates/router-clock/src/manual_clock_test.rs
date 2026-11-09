use super::*;
use std::sync::Arc;

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new("2001-01-01T00:00:00Z".parse().unwrap()))
}

#[test]
fn one_advance_moves_both_readings_and_wakes_all_due_sleepers_without_native_time() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let clock = clock();
        let due = ClockInstant::ZERO.after(Duration::from_secs(10));
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let clock = clock.clone();
            tasks.push(tokio::spawn(async move { clock.sleep_until(due).await }));
        }
        clock.not_due_check_now(due).await;

        clock.advance(Duration::from_secs(9)).unwrap();
        clock.not_due_check_now(due).await;
        assert!(tasks.iter().all(|task| !task.is_finished()));
        clock.advance(Duration::from_secs(1)).unwrap();
        for task in tasks {
            task.await.unwrap();
        }

        assert_eq!(
            clock.utc_now(),
            "2001-01-01T00:00:10Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(clock.now().unix_millis(), 978_307_210_000);
        assert_eq!(clock.monotonic_now(), due);
    });
}

#[test]
fn a_cancelled_sleep_does_not_hold_a_later_advance() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let clock = clock();
        let due = ClockInstant::ZERO.after(Duration::from_secs(1));
        let task = tokio::spawn({
            let clock = clock.clone();
            async move { clock.sleep_until(due).await }
        });
        clock.not_due_check_now(due).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());

        clock.advance(Duration::from_secs(1)).unwrap();
        clock.sleep_until(due).await;

        assert_eq!(
            clock.utc_now(),
            "2001-01-01T00:00:01Z".parse::<DateTime<Utc>>().unwrap()
        );
    });
}

#[test]
fn an_operation_times_out_only_after_the_same_manual_clock_reaches_its_deadline() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let clock = clock();
        let deadline = ClockInstant::ZERO.after(Duration::from_secs(10));
        let task = tokio::spawn({
            let clock = clock.clone();
            async move {
                crate::timeout(
                    &*clock,
                    Duration::from_secs(10),
                    std::future::pending::<()>(),
                )
                .await
            }
        });
        clock.not_due_check_now(deadline).await;

        clock.advance(Duration::from_secs(9)).unwrap();
        clock.not_due_check_now(deadline).await;
        assert!(!task.is_finished());
        clock.advance(Duration::from_secs(1)).unwrap();

        assert!(task.await.unwrap().is_err());
        assert_eq!(
            clock.utc_now(),
            "2001-01-01T00:00:10Z".parse::<DateTime<Utc>>().unwrap()
        );
    });
}

#[test]
fn an_operation_already_ready_at_the_deadline_keeps_its_result() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let clock = clock();
        let result = crate::timeout_at(&*clock, ClockInstant::ZERO, std::future::ready(42)).await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(clock.monotonic_now(), ClockInstant::ZERO);
    });
}

#[test]
fn the_timeout_budget_starts_when_created_even_if_polling_is_delayed() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let clock = clock();
        let waiting = crate::timeout(
            &*clock,
            Duration::from_secs(10),
            std::future::pending::<()>(),
        );
        clock.advance(Duration::from_secs(10)).unwrap();
        assert!(waiting.await.is_err());
        assert_eq!(
            clock.monotonic_now(),
            ClockInstant::ZERO.after(Duration::from_secs(10))
        );
    });
}
