use super::*;

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
    faults.control(RuntimeControl::ClearProjectFault).await?;
    assert!(!faults.project_unavailable()?);
    assert!(!faults.fail_after_writes(1)?);
    Ok(())
}

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
