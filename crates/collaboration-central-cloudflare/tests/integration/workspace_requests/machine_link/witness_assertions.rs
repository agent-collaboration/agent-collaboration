//! Assertions mirror the external oracle's meaning, using only Node snapshots.
use super::super::workerd_host::c2_controls::{
    AttachmentSnapshot, EnrollmentReadOutcome, EnrollmentReadQuery, RestoreOutcome, WitnessEvent,
    WitnessPhase, WitnessRecord,
};
use super::fixture::{MACHINE_ID, ORGANIZATION};
use central_link_protocol::CorrelationId;
use collaboration_central_conformance::ConformanceResult as Result;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn object_id(event: &WitnessEvent) -> &str {
    match event {
        WitnessEvent::Constructor { object_id }
        | WitnessEvent::Upgrade { object_id, .. }
        | WitnessEvent::Handler { object_id, .. }
        | WitnessEvent::EnrollmentRead { object_id, .. }
        | WitnessEvent::RestoreResult { object_id, .. }
        | WitnessEvent::AlarmSnapshot { object_id, .. } => object_id,
    }
}

pub(super) fn valid_records(records: &[WitnessRecord]) -> Result<()> {
    for (index, record) in records.iter().enumerate() {
        require(
            record.sequence == u64::try_from(index)? + 1,
            "witness sequence is not contiguous",
        )?;
        require(
            record.instance_id.get_version_num() == 7,
            "witness instance is not UUIDv7",
        )?;
        let object = object_id(&record.event);
        require(
            object.len() == 64
                && object
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "witness object ID is malformed",
        )?;
        if let WitnessEvent::Upgrade { instance_id, .. }
        | WitnessEvent::Handler { instance_id, .. } = &record.event
        {
            require(
                instance_id == &record.instance_id,
                "witness callback instance differs from recorder",
            )?;
        }
    }
    Ok(())
}

pub(super) fn baseline_attachment(
    records: &[WitnessRecord],
    correlation: &CorrelationId,
) -> Result<AttachmentSnapshot> {
    valid_records(records)?;
    let upgrades = records
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Upgrade { .. }))
        .collect::<Vec<_>>();
    let [upgrade] = upgrades.as_slice() else {
        return Err("baseline needs exactly one real upgrade".into());
    };
    let WitnessEvent::Upgrade { attachments, .. } = &upgrade.event else {
        return Err("upgrade witness missing".into());
    };
    let [attachment] = attachments.as_slice() else {
        return Err("upgrade must retain exactly one attachment".into());
    };
    require(
        attachment.organization_id.as_str() == ORGANIZATION
            && attachment.machine_id.as_str() == MACHINE_ID,
        "attachment public identity changed",
    )?;
    require(
        records.iter().any(|record| {
            record.sequence < upgrade.sequence
                && record.instance_id == upgrade.instance_id
                && matches!(record.event, WitnessEvent::Constructor { .. })
                && object_id(&record.event) == object_id(&upgrade.event)
        }),
        "upgrade needs its real serving constructor",
    )?;
    let handler_count = records
        .iter()
        .filter(|record| {
            record.phase == WitnessPhase::Baseline
                && matches!(record.event, WitnessEvent::Handler { .. })
        })
        .count();
    require(
        handler_count == 1,
        "baseline needs exactly one semantic handler",
    )?;
    let restores = records.iter().filter(|record| matches!(&record.event,WitnessEvent::RestoreResult { correlation_id, outcome:RestoreOutcome::Admitted, .. } if correlation_id == correlation)).collect::<Vec<_>>();
    let [restored] = restores.as_slice() else {
        return Err("baseline admitted restoration witness missing".into());
    };
    alarm_snapshot(
        records,
        correlation,
        restored.instance_id,
        restored.sequence,
    )?;
    Ok(attachment.clone())
}

/// Repeated live read sets are legitimate; one ordered set must precede restoration.
pub(super) fn reconstruction(
    before: &[WitnessRecord],
    after: &[WitnessRecord],
    attachment: &AttachmentSnapshot,
    correlation: &CorrelationId,
    expected: RestoreOutcome,
) -> Result<(uuid::Uuid, u64)> {
    valid_records(after)?;
    require(after.starts_with(before), "witness baseline changed")?;
    let fresh = after
        .get(before.len()..)
        .ok_or("witness baseline length changed")?;
    let constructors = fresh
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Constructor { .. }))
        .collect::<Vec<_>>();
    let handlers = fresh
        .iter()
        .filter(|record| matches!(record.event, WitnessEvent::Handler { .. }))
        .collect::<Vec<_>>();
    let [constructor] = constructors.as_slice() else {
        return Err("deciding frame needs one fresh constructor".into());
    };
    let [handler] = handlers.as_slice() else {
        return Err("deciding frame needs one semantic handler".into());
    };
    require(
        constructor.phase == WitnessPhase::Deciding && handler.phase == WitnessPhase::Deciding,
        "deciding callbacks arrived in another phase",
    )?;
    require(
        constructor.instance_id == handler.instance_id && constructor.sequence < handler.sequence,
        "handler must follow its new constructor",
    )?;
    require(
        !before
            .iter()
            .any(|record| record.instance_id == constructor.instance_id),
        "deciding instance was already serving before eviction",
    )?;
    let WitnessEvent::Handler {
        attachment: restored_attachment,
        ..
    } = &handler.event
    else {
        return Err("semantic handler witness missing".into());
    };
    require(
        restored_attachment == attachment,
        "closed original attachment changed after reconstruction",
    )?;
    let upgrade = before
        .iter()
        .find(|record| matches!(record.event, WitnessEvent::Upgrade { .. }))
        .ok_or("initial upgrade missing")?;
    require(
        object_id(&constructor.event) == object_id(&upgrade.event)
            && object_id(&handler.event) == object_id(&upgrade.event),
        "reconstruction used another organization object",
    )?;
    let restored = fresh.iter().filter(|record| matches!(&record.event,WitnessEvent::RestoreResult { correlation_id, .. } if correlation_id == correlation)).collect::<Vec<_>>();
    let [restored] = restored.as_slice() else {
        return Err("matching restoration witness required exactly once".into());
    };
    let WitnessEvent::RestoreResult { outcome, .. } = &restored.event else {
        return Err("restoration result missing".into());
    };
    require(
        *outcome == expected
            && restored.instance_id == constructor.instance_id
            && restored.sequence > handler.sequence,
        "current live restoration outcome or instance is wrong",
    )?;
    let required = [
        EnrollmentReadQuery::ByEnrollment,
        EnrollmentReadQuery::MachineById,
        EnrollmentReadQuery::ActiveEnrollment,
    ];
    let mut next = 0;
    for record in fresh {
        if record.instance_id == constructor.instance_id
            && record.sequence > handler.sequence
            && record.sequence < restored.sequence
            && let WitnessEvent::EnrollmentRead {
                query,
                outcome: EnrollmentReadOutcome::Read,
                ..
            } = &record.event
            && required.get(next) == Some(query)
        {
            next += 1;
        }
    }
    require(
        next == required.len(),
        "all three actual live enrollment reads must complete in order before restoration",
    )?;
    Ok((constructor.instance_id, restored.sequence))
}

pub(super) fn alarm_snapshot(
    records: &[WitnessRecord],
    correlation: &CorrelationId,
    instance: uuid::Uuid,
    restored_sequence: u64,
) -> Result<()> {
    let alarms = records.iter().filter(|record| matches!(&record.event,WitnessEvent::AlarmSnapshot { correlation_id, .. } if correlation_id == correlation)).collect::<Vec<_>>();
    let [alarm] = alarms.as_slice() else {
        return Err("matching actual alarm snapshot required exactly once".into());
    };
    let WitnessEvent::AlarmSnapshot {
        earliest_due_ms,
        alarm_ms,
        ..
    } = &alarm.event
    else {
        return Err("actual alarm snapshot missing".into());
    };
    require(
        alarm.instance_id == instance && alarm.sequence > restored_sequence,
        "alarm snapshot must follow restoration on the serving instance",
    )?;
    require(
        earliest_due_ms == alarm_ms && alarm_ms.is_none(),
        "actual platform alarm must equal NULL EARLIEST_DUE for this journey",
    )
}
