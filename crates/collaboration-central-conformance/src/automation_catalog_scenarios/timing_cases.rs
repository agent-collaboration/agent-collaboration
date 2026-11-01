//! Central entry applies the same grid and wake lifetime contract as publication.
use super::call;
use crate::{ConformanceResult, native_board_session::NativeBoardSession};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, AutomationValidationError, ExpiryRequest,
    MachineWorkProtocol, MessageContent, MessageDelivery, PullAutomationBatches, SavedMessage,
    TimingRequest, WakeCreator, WakeSendRequest, limits::LimitName,
};

pub(super) async fn run(
    first: &mut NativeBoardSession,
    second: &mut NativeBoardSession,
    target_service: &str,
) -> ConformanceResult<()> {
    for (timing, earlier, later) in [
        (
            TimingRequest::At {
                at: "2026-10-31T10:02:00Z".to_owned().try_into()?,
            },
            "2026-10-31T10:00:00Z",
            "2026-10-31T10:05:00Z",
        ),
        (
            TimingRequest::Interval {
                seconds: 420.try_into()?,
                first_due: None,
            },
            "2026-10-31T10:05:00Z",
            "2026-10-31T10:10:00Z",
        ),
        (
            TimingRequest::After {
                seconds: 600.try_into()?,
            },
            "2026-10-31T10:10:00Z",
            "2026-10-31T10:15:00Z",
        ),
        (
            TimingRequest::Cron {
                expression: "3 * * * *".into(),
                timezone: "UTC".into(),
            },
            "1970-01-01T00:00:00Z",
            "1970-01-01T00:05:00Z",
        ),
    ] {
        let answer = call(
            first,
            AutomationCall::WakeSend(request(target_service, timing, expiry(3600)?)?),
        )
        .await?;
        if !matches!(answer,AutomationAnswer::Refused(AutomationRefusal::Validation {error:AutomationValidationError::OffGrid {earlier:actual_earlier,later:actual_later,..}}) if actual_earlier==earlier&&actual_later==later)
        {
            return Err("central off-grid refusal lost its exact nearest boundaries".into());
        }
    }
    for (timing, end, limit, value) in [
        (
            TimingRequest::Interval {
                seconds: 300.try_into()?,
                first_due: None,
            },
            expiry(3600)?,
            LimitName::WakeMinPeriod,
            300,
        ),
        (
            TimingRequest::Cron {
                expression: "*/5 * * * *".into(),
                timezone: "UTC".into(),
            },
            expiry(3600)?,
            LimitName::WakeMinPeriod,
            300,
        ),
        (
            TimingRequest::At {
                at: "2026-11-01T10:05:00Z".to_owned().try_into()?,
            },
            ExpiryRequest::None,
            LimitName::WakeOneShotMaxAhead,
            86_700,
        ),
        (
            TimingRequest::Interval {
                seconds: 600.try_into()?,
                first_due: None,
            },
            expiry(604_801)?,
            LimitName::WakeRepeatMaxLifetime,
            604_801,
        ),
    ] {
        match call(
            first,
            AutomationCall::WakeSend(request(target_service, timing, end)?),
        )
        .await?
        {
            AutomationAnswer::Refused(AutomationRefusal::Validation {
                error: AutomationValidationError::LimitExceeded(error),
            }) if error.limit == limit && error.value == value => {}
            other => {
                return Err(
                    format!("central timing limit {limit} was not preserved: {other:?}").into(),
                );
            }
        }
    }
    let repeating = TimingRequest::Interval {
        seconds: 600.try_into()?,
        first_due: None,
    };
    if !matches!(
        call(
            first,
            AutomationCall::WakeSend(request(
                target_service,
                repeating.clone(),
                ExpiryRequest::None
            )?)
        )
        .await?,
        AutomationAnswer::Refused(AutomationRefusal::WakeExpiryRequired)
    ) {
        return Err("a repeating Wake accepted an absent expiry".into());
    }
    for (timing, end, expected_end) in [
        (
            TimingRequest::At {
                at: "2026-11-01T10:00:00Z".to_owned().try_into()?,
            },
            ExpiryRequest::None,
            "2026-11-02T10:00:00Z",
        ),
        (repeating, expiry(604_800)?, "2026-11-07T10:00:00Z"),
    ] {
        let AutomationAnswer::Wake(snapshot) = call(
            first,
            AutomationCall::WakeSend(request(target_service, timing, end)?),
        )
        .await?
        else {
            return Err("central refused an exact wake timing bound".into());
        };
        if snapshot.definition.expires_at.map(String::from).as_deref() != Some(expected_end) {
            return Err("exact wake lifetime boundary changed".into());
        }
    }
    publish(second, vec![]).await?;
    publish(first, vec![MachineWorkProtocol::MachineWorkV1]).await?;
    let candidate = request(
        target_service,
        TimingRequest::At {
            at: "2026-10-31T10:05:00Z".to_owned().try_into()?,
        },
        ExpiryRequest::None,
    )?;
    if !matches!(
        call(first, AutomationCall::WakeSend(candidate)).await?,
        AutomationAnswer::Refused(AutomationRefusal::TargetNeedsUpgrade { .. })
    ) {
        return Err(
            "one machine's capability advertisement changed another's withdrawn capability".into(),
        );
    }
    publish(second, vec![MachineWorkProtocol::MachineWorkV1]).await?;
    Ok(())
}

async fn publish(
    machine: &mut NativeBoardSession,
    work_protocols: Vec<MachineWorkProtocol>,
) -> ConformanceResult<()> {
    if !matches!(
        call(
            machine,
            AutomationCall::PullAutomationBatches(PullAutomationBatches {
                after: None,
                max_bytes: 1_048_576,
                work_protocols
            })
        )
        .await?,
        AutomationAnswer::Batches(_)
    ) {
        return Err("capability publication did not complete through pull".into());
    }
    Ok(())
}
fn expiry(seconds: u32) -> ConformanceResult<ExpiryRequest> {
    Ok(ExpiryRequest::After {
        seconds: seconds.try_into()?,
    })
}
fn request(
    service: &str,
    timing: TimingRequest,
    expiry: ExpiryRequest,
) -> ConformanceResult<WakeSendRequest> {
    Ok(WakeSendRequest {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        requested_by: WakeCreator::Owner {},
        replace: false,
        message: SavedMessage {
            target: serde_json::from_value(
                serde_json::json!({"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":format!("wake-limit-{}",durable_steps::Uuid::now_v7())}),
            )?,
            content: MessageContent::HumanUser {
                text: "Bounded central wake".to_owned().try_into()?,
            },
            delivery: MessageDelivery::Auto,
            generation_guard: None,
        },
        timing,
        expiry,
    })
}
