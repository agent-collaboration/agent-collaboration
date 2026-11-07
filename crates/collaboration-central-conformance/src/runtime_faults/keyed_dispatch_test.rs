//! A causal test of independently held invocations and both complete release orders.
use super::*;
use crate::{DispatchCallSelector, DispatchFaultObservation};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Both real barrier futures must remain pending until their own release."
)]
async fn two_invocations_hold_together_and_resume_in_either_order() -> TestResult {
    for reverse in [false, true] {
        let faults = RuntimeFaults::default();
        let destination = ObjectAddress::Copy(
            "019f0000-0000-7000-8000-000000000899"
                .to_owned()
                .try_into()?,
        );
        let fence = DispatchKey::new(destination.clone(), DispatchCallSelector::CopyFence);
        let piece = DispatchKey::new(destination.clone(), DispatchCallSelector::CopyWrite);
        let first_request = br#"{"kind":"fleet-sync.copy-fence"}"#;
        let second_request = br#"{"kind":"central.copy-session-write"}"#;
        for key in [&fence, &piece] {
            faults
                .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
                .await?;
            assert!(matches!(
                faults
                    .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
                    .await,
                Err(RuntimeControlFailure::AlreadyArmed)
            ));
        }
        let first = faults.before_dispatch(&destination, first_request);
        let second = faults.before_dispatch(&destination, second_request);
        tokio::pin!(first, second);
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        for key in [&fence, &piece] {
            let observed = faults
                .control(RuntimeControl::AwaitHeldDispatch { key: key.clone() })
                .await?;
            assert_eq!(observed.dispatches.len(), 2);
            assert!(matches!(
                observed.dispatch(&fence),
                DispatchFaultObservation::Held { .. }
            ));
            assert!(matches!(
                observed.dispatch(&piece),
                DispatchFaultObservation::Held { .. }
            ));
        }
        if reverse {
            faults
                .control(RuntimeControl::ResumeDispatch { key: piece })
                .await?;
            second.await?;
            assert!(futures_util::poll!(&mut first).is_pending());
            faults
                .control(RuntimeControl::ResumeDispatch { key: fence })
                .await?;
            first.await?;
        } else {
            faults
                .control(RuntimeControl::ResumeDispatch { key: fence })
                .await?;
            first.await?;
            assert!(futures_util::poll!(&mut second).is_pending());
            faults
                .control(RuntimeControl::ResumeDispatch { key: piece })
                .await?;
            second.await?;
        }
        // Completion of both calls is the causal end; no timer retires a missed hold.
        assert!(faults.observe()?.dispatches.is_empty());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The fifth independent key must fail without displacing any of the four pending calls."
)]
async fn four_keys_bound_retained_holds_and_each_releases_at_its_own_causal_end() -> TestResult {
    let faults = RuntimeFaults::default();
    let mut keys = Vec::new();
    for _ in 0..5 {
        keys.push(DispatchKey::any(ObjectAddress::Copy(
            fleet_protocol::DatabaseId::new(durable_steps::Uuid::now_v7())?,
        )));
    }
    for key in keys.iter().take(4) {
        faults
            .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
            .await?;
    }
    assert!(matches!(
        faults
            .control(RuntimeControl::HoldNextDispatch {
                key: keys.get(4).ok_or("fifth key missing")?.clone()
            })
            .await,
        Err(RuntimeControlFailure::TooManyDispatchKeys)
    ));
    let mut pending = Vec::new();
    for key in keys.iter().take(4) {
        let mut call = Box::pin(faults.before_dispatch(&key.destination, b"opaque"));
        assert!(futures_util::poll!(&mut call).is_pending());
        pending.push(call);
    }
    assert_eq!(faults.observe()?.dispatches.len(), 4);
    for key in keys.iter().take(4) {
        faults
            .control(RuntimeControl::ResumeDispatch { key: key.clone() })
            .await?;
    }
    for call in pending {
        call.await?;
    }
    assert!(faults.observe()?.dispatches.is_empty());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An invocation matching two armed keys must fail explicitly before consuming either barrier."
)]
async fn overlapping_selections_refuse_the_call_without_consuming_either_key() -> TestResult {
    let faults = RuntimeFaults::default();
    let destination = ObjectAddress::Fleet;
    for call in [DispatchCallSelector::Any, DispatchCallSelector::CopyFence] {
        faults
            .control(RuntimeControl::HoldNextDispatch {
                key: DispatchKey::new(destination.clone(), call),
            })
            .await?;
    }
    assert!(matches!(
        faults
            .before_dispatch(&destination, br#"{"kind":"fleet-sync.copy-fence"}"#)
            .await,
        Err(RuntimeControlFailure::AmbiguousDispatchKey)
    ));
    let observed = faults.observe()?;
    assert_eq!(observed.dispatches.len(), 2);
    assert!(
        observed
            .dispatches
            .iter()
            .all(|hold| matches!(hold.state, DispatchFaultObservation::Armed { .. }))
    );
    Ok(())
}
