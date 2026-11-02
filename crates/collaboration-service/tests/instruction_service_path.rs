use collaboration_protocol::{
    InstructionCreateParams, InstructionShowParams, InstructionText, OperationId,
};
#[path = "support/central_catalog_fixture.rs"]
mod fixture;

#[tokio::test]
async fn real_control_client_creates_and_reads_durable_instruction() -> fixture::ProofResult {
    // The real control client reaches the owning central SQLite with no Local definition store.
    let fixture = fixture::CatalogFixture::open(|transport| transport, false).await?;
    let proof: fixture::ProofResult = async {
    let client = fixture.api.client("instruction-test").await?;
    let request = InstructionCreateParams {
        operation_id: OperationId::generate(),
        text: InstructionText::try_from("Check the workspace".to_owned())?,
    };
    // Act: create via public SDK, replay, and inspect through the same service.
    let first = client.create_instruction(request.clone()).await?;
    let replay = client.create_instruction(request).await?;
    let read = client
        .read_instruction(InstructionShowParams {
            instruction_id: first.instruction_id.clone(),
        })
        .await?;
    // Assert: actual RPC reaches durable storage and preserves operation identity.
    if first.instruction_id != replay.instruction_id || read.text.as_str() != "Check the workspace"
    {
        return Err("instruction service path did not preserve content/identity".into());
    }
    let update = collaboration_protocol::InstructionUpdateParams {
        operation_id: OperationId::generate(),
        instruction_id: first.instruction_id.clone(),
        expected_revision_id: first.revision_id.clone(),
        text: InstructionText::try_from("Updated instructions".to_owned())?,
    };
    let updated = client.update_instruction(update.clone()).await?;
    let stale = collaboration_protocol::InstructionUpdateParams {
        operation_id: OperationId::generate(),
        ..update
    };
    let error = client
        .update_instruction(stale)
        .await
        .err()
        .ok_or("stale update accepted")?;
    let collaboration_client::InstructionClientError::Catalog(collaboration_client::CentralCatalogFailure::Refused { error, .. }) = error else {
        return Err("stale update lacked typed conflict".into());
    };
    if !matches!(*error, collaboration_protocol::AutomationRefusal::InstructionRevisionConflict { current_revision_id } if current_revision_id == updated.revision_id) {
        return Err(
            "revision conflict omitted the revision observed by the rejecting transaction".into(),
        );
    }
    Ok(())
    }.await;
    let stopped = fixture.close().await;
    proof?;
    stopped
}
