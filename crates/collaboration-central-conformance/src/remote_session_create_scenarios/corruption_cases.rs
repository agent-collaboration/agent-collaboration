//! Each independently decoded stored payload refuses through normal contracts.
use super::{call, fixture, pull_work, session_create_request};
use crate::{
    ConformanceClient, ConformanceResult, native_board_session::NativeBoardSession,
    scenario_assertions::equal,
};
use collaboration_central_core::MachineWorkFixtureMutation;
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationCall, AutomationRefusal,
    BatchAcknowledgement, FirstInputState, RemoteSessionCreateRequest, ReportWorkItems,
    SessionCreateState, WorkItemId, WorkItemReport,
};
use serde_json::{Value, json};

pub(super) async fn run(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    source_service: &str,
    target_service: &str,
) -> ConformanceResult<()> {
    let request = session_create_request(source_service, target_service)?;
    let admitted = match call(source, AutomationCall::RemoteSessionCreate(request.clone())).await? {
        AutomationAnswer::SessionCreate(row) => row,
        other => return Err(format!("corruption setup admission refused: {other:?}").into()),
    };
    let work_item = &admitted.work_item_id;
    // A named fixture state prevents legitimate pending→held notice progress
    // from obscuring raw no-write assertions; this is not custody proof.
    let original = fixture::machine_work(
        client,
        work_item,
        Some(MachineWorkFixtureMutation::State {
            state: "held".into(),
        }),
    )
    .await?;
    let request_json: String = fixture::field(&original, "/requestJson")?;
    let request_digest: String = fixture::field(&original, "/requestDigest")?;
    let result_json: Option<String> = fixture::field(&original, "/resultJson")?;
    let mut malformed: Value = serde_json::from_str(&request_json)?;
    *malformed
        .pointer_mut("/request/endpoint/serviceId")
        .ok_or("stored endpoint missing")? = json!("invalid-service-identity");
    for (mutation, restoration, field) in [
        (
            MachineWorkFixtureMutation::RequestJson {
                request_json: malformed.to_string(),
            },
            MachineWorkFixtureMutation::RequestJson {
                request_json: request_json.clone(),
            },
            "machine_work_items.request_json",
        ),
        (
            MachineWorkFixtureMutation::RequestDigest {
                request_digest: "0".repeat(64),
            },
            MachineWorkFixtureMutation::RequestDigest { request_digest },
            "machine_work_items.request_digest",
        ),
        (
            MachineWorkFixtureMutation::State {
                state: "future-custody".into(),
            },
            MachineWorkFixtureMutation::State {
                state: "held".into(),
            },
            "machine_work_items.state",
        ),
        (
            MachineWorkFixtureMutation::ResultJson {
                result_json: Some(
                    json!({"seq":0,
            "create":{"state":"unknown"},"input":{"state":"unknown"}})
                    .to_string(),
                ),
            },
            MachineWorkFixtureMutation::ResultJson { result_json },
            "machine_work_items.result_json",
        ),
    ] {
        let invalid = fixture::machine_work(client, work_item, Some(mutation)).await?;
        let proof =
            refuse_corrupt_row(client, source, target, &request, work_item, &invalid, field).await;
        let restored = fixture::machine_work(client, work_item, Some(restoration)).await;
        proof?;
        equal(restored?, original.clone())?;
    }
    let batch_id: collaboration_protocol::BatchId =
        fixture::field(&original, "/batches/0/batchId")?;
    let original_bytes: String = fixture::field(&original, "/batches/0/frozenBytes")?;
    let mut changed: Value = serde_json::from_str(&original_bytes)?;
    *changed
        .pointer_mut("/entries/0/value/request/workingDirectory")
        .ok_or("frozen request missing")? = json!("/different-frozen-request");
    let invalid = fixture::machine_work(
        client,
        work_item,
        Some(MachineWorkFixtureMutation::FrozenBatch {
            batch_id: batch_id.clone(),
            frozen_bytes: changed.to_string(),
        }),
    )
    .await?;
    let digest = fixture::field(&invalid, "/batches/0/digest")?;
    let proof = async {
        invalid_record(
            call(
                target,
                AutomationCall::PullAutomationBatches(
                    collaboration_protocol::PullAutomationBatches {
                        after: None,
                        max_bytes: 1_048_576,
                        work_protocols: vec![
                            collaboration_protocol::MachineWorkProtocol::MachineWorkV1,
                        ],
                    },
                ),
            )
            .await?,
            "batch.work_item",
        )?;
        equal(
            fixture::machine_work(client, work_item, None).await?,
            invalid.clone(),
        )?;
        invalid_record(
            call(
                target,
                AutomationCall::AckAutomationBatches(AckAutomationBatches {
                    batches: vec![BatchAcknowledgement {
                        batch_id: batch_id.clone(),
                        digest,
                    }],
                }),
            )
            .await?,
            "batch.work_item",
        )?;
        equal(
            fixture::machine_work(client, work_item, None).await?,
            invalid.clone(),
        )?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let restored = fixture::machine_work(
        client,
        work_item,
        Some(MachineWorkFixtureMutation::FrozenBatch {
            batch_id,
            frozen_bytes: original_bytes,
        }),
    )
    .await;
    proof?;
    equal(restored?, original)?;
    pull_work(target).await?;
    println!(
        "REMOTE_CREATE_CORRUPTION_RESULT {}",
        json!({
            "requestAndResultPayloads":true,"digestAndState":true,"frozenBindingWithValidDigest":true,
            "normalStatusPullAckReport":true,"rawInvalidBytesAndCustodyUnchanged":true,
            "heldState":"explicit fixture setup, not custody proof"
        })
    );
    Ok(())
}

async fn refuse_corrupt_row(
    client: &ConformanceClient,
    source: &mut NativeBoardSession,
    target: &mut NativeBoardSession,
    request: &RemoteSessionCreateRequest,
    work_item: &WorkItemId,
    invalid: &Value,
    field: &str,
) -> ConformanceResult<()> {
    invalid_record(
        call(
            source,
            AutomationCall::RemoteSessionCreateStatus {
                operation_id: request.operation_id.clone(),
            },
        )
        .await?,
        field,
    )?;
    let after = fixture::machine_work(client, work_item, None).await?;
    println!(
        "REMOTE_CREATE_CORRUPTION_RAW_OBSERVATION {}",
        json!({"field":field,"invalid":invalid,"afterStatus":after})
    );
    equal(after, invalid.clone())?;
    invalid_record(
        call(
            target,
            AutomationCall::PullAutomationBatches(collaboration_protocol::PullAutomationBatches {
                after: None,
                max_bytes: 1_048_576,
                work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
            }),
        )
        .await?,
        field,
    )?;
    equal(
        fixture::machine_work(client, work_item, None).await?,
        invalid.clone(),
    )?;
    invalid_record(
        call(
            target,
            AutomationCall::ReportWorkItems(ReportWorkItems::new(vec![
                WorkItemReport::SessionCreate {
                    work_item_id: work_item.clone(),
                    seq: 1.try_into()?,
                    create: Box::new(SessionCreateState::Unknown),
                    input: FirstInputState::None,
                },
            ])?),
        )
        .await?,
        field,
    )?;
    equal(
        fixture::machine_work(client, work_item, None).await?,
        invalid.clone(),
    )?;
    Ok(())
}

fn invalid_record(answer: AutomationAnswer, expected: &str) -> ConformanceResult<()> {
    match answer {
        AutomationAnswer::Refused(AutomationRefusal::InvalidRecord { field })
            if field == expected =>
        {
            Ok(())
        }
        other => Err(format!("corrupt {expected} lost its typed refusal: {other:?}").into()),
    }
}
