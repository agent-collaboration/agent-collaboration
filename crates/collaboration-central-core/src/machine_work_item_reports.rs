//! Target reports advance their sequence while retaining already-known outcomes.
use crate::{
    automation_store_error::{AutomationStoreError, AutomationStoreResult},
    machine_work_item_record::MachineWorkResult,
    machine_work_item_store as items,
};
use collaboration_protocol::{
    AutomationRefusal, FirstInputState, SessionCreateState, WorkItemId, WorkReportSequence,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn validate<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &ServiceId,
    id: &WorkItemId,
    create: &SessionCreateState,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let item = items::read(scope, id)
        .await?
        .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
    if item.target != *installation {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::OriginMismatch,
        ));
    }
    if let SessionCreateState::Created { session_ref, .. } = create
        && String::from(session_ref.endpoint.service_id.clone()) != installation.as_str()
    {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::OriginMismatch,
        ));
    }
    Ok(())
}

pub(crate) async fn apply<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &WorkItemId,
    seq: WorkReportSequence,
    create: &SessionCreateState,
    input: &FirstInputState,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    // Earlier items in this same envelope may have advanced this subject.
    let item = items::read(scope, id)
        .await
        .map_err(StoreError::backend)?
        .ok_or_else(|| StoreError::backend("validated machine-work item disappeared"))?;
    if let Some(result) = advance_result(item.result.as_ref(), seq, create, input) {
        items::write_result(scope, id, &result).await?;
    }
    Ok(())
}

fn advance_result(
    previous: Option<&MachineWorkResult>,
    seq: WorkReportSequence,
    create: &SessionCreateState,
    input: &FirstInputState,
) -> Option<MachineWorkResult> {
    if previous.is_some_and(|previous| u64::from(previous.seq) >= u64::from(seq)) {
        return None;
    }
    let known_create = previous.and_then(|previous| match &previous.create {
        SessionCreateState::Unknown => None,
        known @ (SessionCreateState::Created { .. } | SessionCreateState::Refused { .. }) => {
            Some(known)
        }
    });
    let final_input = previous.and_then(|previous| match &previous.input {
        known @ (FirstInputState::Accepted | FirstInputState::Failed { .. }) => Some(known),
        FirstInputState::None | FirstInputState::Unknown => None,
    });
    Some(MachineWorkResult {
        seq,
        create: known_create.unwrap_or(create).clone(),
        input: final_input.unwrap_or(input).clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn newer_unknown_report_retains_known_fields_and_advances_sequence() {
        for (create, input) in [
            (
                json!({"state":"refused","reason":{"kind":"approverRequired"}}),
                json!({"state":"failed","reason":"first input refused"}),
            ),
            (
                json!({"state":"created","sessionRef":{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000004","endpointId":"codex-local"},"sessionId":"known"},"settings":{"state":"applied","effectiveSettings":null,"permissionDifferences":[]}}),
                json!({"state":"accepted"}),
            ),
        ] {
            let previous: MachineWorkResult =
                serde_json::from_value(json!({"seq":3,"create":create,"input":input}))
                    .expect("known result");
            let next = advance_result(
                Some(&previous),
                4.try_into().expect("sequence"),
                &SessionCreateState::Unknown,
                &FirstInputState::Unknown,
            )
            .expect("newer report");
            assert_eq!(u64::from(next.seq), 4);
            assert_eq!(serde_json::to_value(next.create).expect("create"), create);
            assert_eq!(serde_json::to_value(next.input).expect("input"), input);
            for seq in [2, 3] {
                assert!(
                    advance_result(
                        Some(&previous),
                        seq.try_into().expect("sequence"),
                        &SessionCreateState::Unknown,
                        &FirstInputState::Unknown
                    )
                    .is_none()
                );
            }
        }
    }

    #[test]
    fn unreported_and_reported_unknown_can_become_known() {
        let create = SessionCreateState::Refused {
            reason: collaboration_protocol::RemoteCreateRefusal::ApproverNotSupported,
        };
        let previous = MachineWorkResult {
            seq: 1.try_into().expect("sequence"),
            create: SessionCreateState::Unknown,
            input: FirstInputState::Unknown,
        };
        for before in [None, Some(&previous)] {
            let next = advance_result(
                before,
                2.try_into().expect("sequence"),
                &create,
                &FirstInputState::None,
            )
            .expect("new report");
            assert_eq!(u64::from(next.seq), 2);
            assert!(matches!(
                next.create,
                SessionCreateState::Refused {
                    reason: collaboration_protocol::RemoteCreateRefusal::ApproverNotSupported
                }
            ));
            assert!(matches!(next.input, FirstInputState::None));
        }
    }
}
