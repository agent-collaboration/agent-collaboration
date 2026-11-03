use super::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions must fail the test while fixture errors propagate."
)]
#[tokio::test]
async fn a_project_outage_stays_active_for_repeated_reads_and_writes_until_cleared()
-> Result<(), RuntimeControlFailure> {
    // Arrange: the new mode differs from the existing one-shot CopyFull fault.
    let faults = RuntimeFaults::default();
    faults.control(RuntimeControl::ProjectUnavailable).await?;
    // Act / Assert: even read-only transactions remain unavailable on each call.
    for written in [0, 1, 2, 0] {
        assert!(faults.project_unavailable()?);
        assert!(faults.fail_after_writes(written)?);
    }
    assert!(matches!(
        faults
            .control(RuntimeControl::CopyFull {
                after_writes: std::num::NonZeroU64::MIN
            })
            .await,
        Err(RuntimeControlFailure::AlreadyArmed)
    ));
    assert!(faults.project_unavailable()?);
    faults.control(RuntimeControl::ClearProjectFault).await?;
    assert!(!faults.project_unavailable()?);
    assert!(!faults.fail_after_writes(1)?);
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions must fail the test while fixture errors propagate."
)]
#[tokio::test]
async fn the_one_shot_storage_fault_keeps_its_existing_trigger_semantics()
-> Result<(), RuntimeControlFailure> {
    let faults = RuntimeFaults::default();
    faults
        .control(RuntimeControl::CopyFull {
            after_writes: std::num::NonZeroU64::MIN,
        })
        .await?;
    assert!(!faults.fail_after_writes(0)?);
    assert!(faults.fail_after_writes(1)?);
    assert!(!faults.fail_after_writes(1)?);
    assert!(!faults.project_unavailable()?);
    Ok(())
}

#[test]
fn project_fault_controls_refuse_the_fleet_owner() {
    for command in [
        RuntimeControl::ProjectUnavailable,
        RuntimeControl::ClearProjectFault,
    ] {
        let request = crate::RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command,
        };
        assert!(matches!(
            request.validate(),
            Err(RuntimeControlFailure::WrongObjectKind)
        ));
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while invalid fixture identities propagate."
)]
fn fleet_submit_controls_require_the_fleet_owner_and_destination()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::{DispatchCallSelector, RuntimeControlRequest};
    let peer = "019f0000-0000-7000-8000-000000000810"
        .to_owned()
        .try_into()?;
    let item = "019f0000-0000-7000-8000-000000000811"
        .to_owned()
        .try_into()?;
    let non_fleet = ObjectAddress::Copy(
        "019f0000-0000-7000-8000-000000000812"
            .to_owned()
            .try_into()?,
    );
    for call in [
        DispatchCallSelector::FleetMailSubmit,
        DispatchCallSelector::FleetMailSubmitFor { peer, item },
    ] {
        for destination in [ObjectAddress::Fleet, non_fleet.clone()] {
            for command in [
                RuntimeControl::HoldNextSelectedDispatch {
                    destination: destination.clone(),
                    call,
                },
                RuntimeControl::ResumeDispatchAndHoldNextSelected {
                    destination: destination.clone(),
                    call,
                },
                RuntimeControl::DropNextSelectedObjectReply {
                    destination: destination.clone(),
                    call,
                },
            ] {
                for address in [ObjectAddress::Fleet, non_fleet.clone()] {
                    let request = RuntimeControlRequest {
                        address: address.clone(),
                        command: command.clone(),
                    };
                    assert_eq!(
                        request.validate().is_ok(),
                        matches!(
                            (&address, &destination),
                            (ObjectAddress::Fleet, ObjectAddress::Fleet)
                        )
                    );
                }
            }
        }
    }
    assert!(
        serde_json::from_value::<ObjectAddress>(
            serde_json::json!({"kind":"mailbox","value":"019f0000-0000-7000-8000-000000000812"})
        )
        .is_err()
    );
    for retired in [
        serde_json::json!("mailboxSubmit"),
        serde_json::json!({"mailboxSubmitFor":{"origin":peer,"item":item}}),
    ] {
        assert!(serde_json::from_value::<DispatchCallSelector>(retired).is_err());
    }
    Ok(())
}
