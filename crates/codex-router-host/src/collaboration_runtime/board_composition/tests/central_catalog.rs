//! Public catalog calls cross the actual Host, API socket, machine session and central store.
use super::*;
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationPageRequest, InstructionCreateParams,
    InstructionShowParams, InstructionUpdateParams, OperationId, RevisionListRequest,
};
use collaboration_runtime::CentralAutomationTransport;

use super::central_catalog_fixture::{CatalogHosts, ProofResult, connected_transport};

#[tokio::test]
async fn public_instruction_create_reaches_central_and_is_readable_on_another_host() -> TestResult {
    // Arrange: two actual enrolled Hosts; no native process or model input.
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        let first_transport = connected_transport(&hosts.first).await?;
        let second_transport = connected_transport(&hosts.second).await?;
        drop(first_transport);
        let first_client =
            CollaborationClient::connect(&hosts.first_root, "catalog-a", "1").await?;
        let second_client =
            CollaborationClient::connect(&hosts.second_root, "catalog-b", "1").await?;
        let request = InstructionCreateParams {
            operation_id: OperationId::generate(),
            text: "One organization-wide instruction".to_owned().try_into()?,
        };

        // Act: create through A's public API, then inspect central through B's session.
        let created = first_client.create_instruction(request.clone()).await?;
        let shown = second_transport
            .call(AutomationCall::InstructionShow(InstructionShowParams {
                instruction_id: created.instruction_id.clone(),
            }))
            .await?;
        let AutomationAnswer::Instruction(central_document) = shown else {
            return Err("public instruction create never reached central".into());
        };

        // Assert: B's public API and a replay return exactly the same document/revision.
        let read = second_client
            .read_instruction(InstructionShowParams {
                instruction_id: created.instruction_id.clone(),
            })
            .await?;
        let replayed = first_client.create_instruction(request).await?;
        let expected = serde_json::to_value(&created)?;
        for document in [central_document, read, replayed] {
            if serde_json::to_value(document)? != expected {
                return Err(
                    "catalog identity, revision or text changed across Hosts/replay".into(),
                );
            }
        }
        use sqlx::Connection;
        for root in [&hosts.first_root, &hosts.second_root] {
            let mut sql = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new().filename(root.join("automation.sqlite")),
            )
            .await?;
            let local: i64 = sqlx::query_scalar("SELECT count(*) FROM instruction_documents")
                .fetch_one(&mut sql)
                .await?;
            sql.close().await?;
            if local != 0 {
                return Err(
                    "a public catalog call still created a local instruction definition".into(),
                );
            }
        }
        Ok(())
    }
    .await;
    let stopped = hosts.close().await;
    proof?;
    stopped?;
    Ok(())
}

#[tokio::test]
async fn public_instruction_pages_keep_central_rows_and_opaque_cursors() -> TestResult {
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let first = CollaborationClient::connect(&hosts.first_root, "catalog-page-a", "1").await?;
        let second =
            CollaborationClient::connect(&hosts.second_root, "catalog-page-b", "1").await?;
        let mut expected = std::collections::BTreeSet::new();
        for text in ["First central document", "Second central document"] {
            let document = first
                .create_instruction(InstructionCreateParams {
                    operation_id: OperationId::generate(),
                    text: text.to_owned().try_into()?,
                })
                .await?;
            expected.insert(document.instruction_id.as_str().to_owned());
        }
        let mut request = AutomationPageRequest {
            cursor: None,
            limit: 1.try_into()?,
        };
        let mut observed = std::collections::BTreeSet::new();
        for ordinal in 0..2 {
            let AutomationAnswer::Instructions(central_page) = transport
                .call(AutomationCall::InstructionList(request.clone()))
                .await?
            else {
                return Err("central instruction page missing".into());
            };
            let page = if ordinal == 0 {
                second.list_instructions(request.clone()).await?
            } else {
                first.list_instructions(request.clone()).await?
            };
            if serde_json::to_value(&page)? != serde_json::to_value(&central_page)? {
                return Err("public instruction page replaced central rows or cursor".into());
            }
            if page.records.len() != 1 {
                return Err("instruction page lost its requested bound".into());
            }
            for document in &page.records {
                observed.insert(document.instruction_id.as_str().to_owned());
            }
            match (ordinal, page.next_cursor) {
                (0, Some(cursor)) => request.cursor = Some(cursor),
                (1, None) => {}
                _ => return Err("instruction cursor ended early or invented another page".into()),
            }
        }
        if observed != expected {
            return Err("instruction pagination lost or duplicated a document".into());
        }
        let invalid = second
            .list_instructions(AutomationPageRequest {
                cursor: Some("not-a-central-cursor".to_owned()),
                limit: 1.try_into()?,
            })
            .await;
        if !matches!(invalid,
            Err(collaboration_client::AutomationInspectionClientError::Catalog(
                collaboration_client::CentralCatalogFailure::Refused { operation_id: None, error, .. }
            )) if matches!(&*error, collaboration_protocol::AutomationRefusal::InvalidRequest {
                field, reason: collaboration_protocol::AutomationRequestConstraint::InvalidCursor
            } if field == "cursor")
        ) {
            return Err("central cursor refusal was replaced by a local error".into());
        }
        Ok(())
    }
    .await;
    let stopped = hosts.close().await;
    proof?;
    stopped?;
    Ok(())
}

#[tokio::test]
async fn public_revision_pages_keep_central_revisions_and_revision_conflict() -> TestResult {
    use collaboration_client::{CentralCatalogFailure, InstructionClientError};
    use collaboration_protocol::AutomationRefusal;
    let hosts = CatalogHosts::open().await?;
    let proof: ProofResult = async {
        drop(connected_transport(&hosts.first).await?);
        let transport = connected_transport(&hosts.second).await?;
        let first = CollaborationClient::connect(&hosts.first_root, "catalog-revision-a", "1").await?;
        let second = CollaborationClient::connect(&hosts.second_root, "catalog-revision-b", "1").await?;
        let created = first.create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(), text: "Original central revision".to_owned().try_into()?,
        }).await?;
        let updated = second.update_instruction(InstructionUpdateParams {
            operation_id: OperationId::generate(), instruction_id: created.instruction_id.clone(),
            expected_revision_id: created.revision_id.clone(), text: "Updated from the other Host".to_owned().try_into()?,
        }).await?;
        let mut request = RevisionListRequest {
            instruction_id: created.instruction_id.clone(), cursor: None, limit: 1.try_into()?,
        };
        let mut observed = std::collections::BTreeSet::new();
        for ordinal in 0..2 {
            let AutomationAnswer::Revisions(central_page) = transport.call(
                AutomationCall::RevisionList(request.clone()),
            ).await? else { return Err("central revision page missing".into()); };
            let page = second.list_instruction_revisions(request.clone()).await
                .map_err(|error| format!("public revision page did not read central: {error}"))?;
            if serde_json::to_value(&page)? != serde_json::to_value(&central_page)? {
                return Err("public revision page replaced central rows or cursor".into());
            }
            if page.records.len() != 1 { return Err("revision page lost its requested bound".into()); }
            for revision in &page.records { observed.insert(revision.revision_id.as_str().to_owned()); }
            match (ordinal, page.next_cursor) {
                (0, Some(cursor)) => request.cursor = Some(cursor),
                (1, None) => {},
                _ => return Err("revision cursor ended early or invented another page".into()),
            }
        }
        let expected = [created.revision_id.as_str().to_owned(), updated.revision_id.as_str().to_owned()]
            .into_iter().collect();
        if observed != expected { return Err("revision pagination lost or duplicated history".into()); }
        let operation = OperationId::generate();
        let stale = first.update_instruction(InstructionUpdateParams {
            operation_id: operation.clone(), instruction_id: created.instruction_id.clone(),
            expected_revision_id: created.revision_id, text: "Stale write must be refused".to_owned().try_into()?,
        }).await;
        match stale {
            Err(InstructionClientError::Catalog(CentralCatalogFailure::Refused { operation_id: Some(actual), error, .. }))
                if actual == operation && matches!(&*error, AutomationRefusal::InstructionRevisionConflict { current_revision_id } if current_revision_id == &updated.revision_id) => {},
            _ => return Err("central revision conflict lost its actual head or operation ID".into()),
        }
        let shown = first.read_instruction(InstructionShowParams { instruction_id: created.instruction_id }).await?;
        if serde_json::to_value(shown)? != serde_json::to_value(updated)? {
            return Err("stale mutation changed the central instruction head".into());
        }
        Ok(())
    }.await;
    let stopped = hosts.close().await;
    proof?;
    stopped?;
    Ok(())
}
