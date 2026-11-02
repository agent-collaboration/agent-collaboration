//! Public inspection reads actual centrally admitted Firings; no native backend executes work.
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationCall, BatchAcknowledgement,
    FiringListRequest, FiringSnapshot, MachineWorkProtocol, OperationId, PullAutomationBatches,
};
use serde_json::json;
#[path = "support/central_catalog_fixture.rs"]
pub mod fixture;
use fixture::{CatalogFixture, ProofResult, SERVICE};

async fn advance(fixture: &CatalogFixture, now: &str) -> ProofResult {
    fixture
        .runtime
        .runtime_control(
            board_central_test_support::ObjectAddress::Fleet,
            serde_json::from_value(
                json!({"kind":"automationClock","value":{"kind":"advance","now":now}}),
            )?,
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn public_firing_inspection_preserves_shared_snapshots_and_central_page_boundaries()
-> ProofResult {
    let fixture = CatalogFixture::open_with_controls(
        |transport| transport,
        false,
        vec![serde_json::from_value(
            json!({"address":{"kind":"fleet"},"command":{
                "kind":"automationClock","value":{"kind":"start","now":"2026-11-01T12:00:00Z"}
            }}),
        )?],
    )
    .await?;
    let proof: ProofResult = async {
        let ready = fixture.transport.call(AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None, max_bytes: 1_048_576, work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        })).await?;
        if !matches!(ready, AutomationAnswer::Batches(_)) { return Err("fixture capability not advertised".into()); }
        let client = fixture.api.client("central-firing-inspection").await?;
        let instruction = client.create_instruction(collaboration_protocol::InstructionCreateParams {
            operation_id: OperationId::generate(), text: "Never executed: central inspection fixture".to_owned().try_into()?,
        }).await?;
        let schedule = client.create_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"definition":{
                "instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},"enabled":true,
                "destination":{"kind":"ownedThread","target":{"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"inert-firing-inspection"},"cwd":"/tmp"},
                "executionTimeoutSeconds":null,"model":"gpt-6-luna","effort":"low"
            }
        }))?).await?;
        advance(&fixture,"2026-11-01T12:05:00Z").await?;
        let AutomationAnswer::Batches(custody) = fixture.transport.call(AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None, max_bytes: 1_048_576, work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        })).await? else { return Err("actual central custody missing".into()); };
        if custody.batches.len() != 1 { return Err("first due did not admit exactly one actual Batch".into()); }
        let ack = fixture.transport.call(AutomationCall::AckAutomationBatches(AckAutomationBatches {
            batches: custody.batches.into_iter().map(|batch| BatchAcknowledgement { batch_id: batch.batch_id, digest: batch.digest }).collect(),
        })).await?;
        if !matches!(ack,AutomationAnswer::Acknowledged) { return Err("actual custody ACK refused".into()); }
        // One received occupant and one waiting Firing; no target execution/report is invented.
        advance(&fixture,"2026-11-01T12:15:00Z").await?;
        let automation_id = schedule.schedule_id.as_str().to_owned().try_into()?;
        let mut request = FiringListRequest { automation_id, cursor: None, limit: 1_u32.try_into()? };
        let mut observed = Vec::<FiringSnapshot>::new();
        for ordinal in 0..2 {
            let AutomationAnswer::Firings(expected) = fixture.transport.call(AutomationCall::FiringList(request.clone())).await?
                else { return Err("actual central Firing page missing".into()); };
            if expected.records.len() != 1 || expected.next_cursor.is_some() != (ordinal == 0) {
                return Err("actual controlled fixture did not expose two central pages".into());
            }
            let firing = &expected.records[0];
            let shown = fixture.api.call("firing_show",json!({"firingId":firing.firing_id})).await?;
            if shown.get("result") != Some(&serde_json::to_value(firing)?) {
                return Err(format!("public FiringShow did not preserve actual central snapshot: {shown}").into());
            }
            let listed = fixture.api.call("firing_list",serde_json::to_value(&request)?).await?;
            if listed.get("result") != Some(&serde_json::to_value(&expected)?) {
                return Err("public FiringList changed rows or central continuation".into());
            }
            let sdk_shown = client.read_firing(collaboration_client::FiringShowRequest { firing_id: firing.firing_id.clone() }).await?;
            let sdk_listed = client.list_firings(request.clone()).await?;
            if serde_json::to_value(&sdk_shown)? != serde_json::to_value(firing)?
                || serde_json::to_value(&sdk_listed)? != serde_json::to_value(&expected)? {
                return Err("typed SDK changed shared Firing evidence or central cursor".into());
            }
            observed.extend(expected.records);
            request.cursor = expected.next_cursor;
        }
        if observed[0].firing_id == observed[1].firing_id
            || observed.iter().any(|firing| firing.report_seq.is_some()) {
            return Err("inspection repeated identity or invented a machine report".into());
        }
        Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
