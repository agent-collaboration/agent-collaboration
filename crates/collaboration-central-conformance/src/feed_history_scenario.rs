//! Ordinary writes and feed reads preserve immutable snapshots after canonical changes.
use crate::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardReadCursor, BoardReadPage, BoardReadScope,
    FleetFeedRead,
};
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::{
    AdrId, AdrStatus, AdrTransition, ArtifactRef, ChangeOutcome, FleetPosition, ProtocolVersion,
    Record, RecordedActor, Refusal, RepositoryRef, RequestDecision, RequestId, SubjectRevision,
    WorkspaceId,
};

fn request(workspace: &WorkspaceId, operation: FleetOperation) -> ConformanceResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace: workspace.clone(),
        actor: RecordedActor::Human {
            human_id: "historical owner".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
async fn changed(
    session: &mut NativeBoardSession,
    request: &FleetRequest,
) -> ConformanceResult<RequestDecision> {
    let result = session.write(request).await?;
    if !matches!(
        &result,
        RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        }
    ) {
        return Err(format!("history mutation was not committed: {result:?}").into());
    }
    Ok(result)
}
async fn feed(session: &mut NativeBoardSession) -> ConformanceResult<(Vec<Record>, u64)> {
    let mut after = FleetPosition::ZERO;
    let mut cut = None;
    let mut entries = Vec::new();
    loop {
        let BoardConsumerAnswer::Read(BoardReadPage::FleetFeed(FleetFeedRead::Page {
            records,
            through,
            head,
            ..
        })) = session
            .request(BoardConsumerRequest::Read {
                scope: BoardReadScope::FleetRecords,
                cursor: Some(BoardReadCursor::Fleet(after)),
            })
            .await?
        else {
            return Err("history read returned another page family".into());
        };
        if cut.is_some_and(|cut| cut != head) {
            return Err("isolated history head changed between pages".into());
        }
        cut = Some(head);
        for record in records {
            let record = record.decode::<Record>()?;
            let expected = u64::try_from(entries.len())?
                .checked_add(1)
                .ok_or("history fixture position overflow")?;
            if record
                .envelope()
                .position
                .fleet_position()
                .map(|position| position.get())
                != Some(expected)
            {
                return Err("history lost its exact committed ordering".into());
            }
            entries.push(record);
        }
        if through == head {
            return Ok((entries, head.get()));
        }
        if through <= after {
            return Err("history continuation made no progress".into());
        }
        after = through;
    }
}

pub async fn run_feed_history_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut session = NativeBoardSession(
        enroll_admitted_peer(
            client,
            "019f0000-0000-7000-8000-000000009fa1",
            "019f0000-0000-7000-8000-000000009fa2",
            "typed-feed-session",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let proof=async {
        let workspace=WorkspaceId::generate();
        changed(&mut session,&request(&workspace,FleetOperation::WorkspaceCreate {name:"Original history workspace".to_owned().try_into()?,description:"original description".to_owned().try_into()?})?).await?;
        let repositories=(0..16).map(|ordinal|Ok(RepositoryRef::Origin {normalized_origin:format!("example.test/history-{ordinal:02}").try_into()?})).collect::<ConformanceResult<Vec<_>>>()?;
        for repository in repositories.iter().rev(){
            changed(&mut session,&request(&workspace,FleetOperation::RepositoryAttach {repository:repository.clone()})?).await?;
        }
        let adr=AdrId::generate();
        let files=(0..16).map(|ordinal|Ok(ArtifactRef {
            repository:"example.test/history".to_owned().try_into()?,
            path:format!("literal/{}-λ\0tail",15-ordinal),
            commit:if ordinal%2==0 {Some("ab".repeat(20).try_into()?)}else{None},
        })).collect::<ConformanceResult<Vec<_>>>()?;
        let create=request(&workspace,FleetOperation::AdrCreate {adr:adr.clone(),title:"Original historical ADR".to_owned().try_into()?,text:"Literal **historical** body".to_owned().try_into()?,supersedes:None,made_in:None,repository_files:files.clone()})?;
        changed(&mut session,&create).await?;
        let before=feed(&mut session).await?;
        if before.1!=36 || before.0.len()!=36{return Err("fixture did not publish exactly eighteen resource/ledger pairs".into());}
        let original_workspace=before.0.iter().rev().find_map(|record|match record {Record::Workspace(value) if value.workspace_id==workspace=>Some(value),_=>None}).ok_or("history omitted Workspace")?;
        if original_workspace.name.as_str()!="Original history workspace" || original_workspace.repositories!=repositories{return Err("historical repository order differs from its literal canonical vector".into());}
        let original_adr=before.0.iter().find_map(|record|match record {Record::Adr(value) if value.adr_id==adr=>Some(value),_=>None}).ok_or("history omitted ADR")?;
        if original_adr.status!=AdrStatus::Proposed || original_adr.repository_files!=files || original_adr.envelope.actor!=create.actor || original_adr.envelope.request.as_ref()!=Some(&create.request_id){return Err("historical ADR files or complete attributed envelope differ".into());}
        let detached=repositories.first().cloned().ok_or("missing repository fixture")?;
        let remaining_repositories=repositories.iter().skip(1).cloned().collect::<Vec<_>>();
        let changes=[
            request(&workspace,FleetOperation::WorkspaceUpdate {name:"Later canonical workspace".to_owned().try_into()?,description:"later description".to_owned().try_into()?})?,
            request(&workspace,FleetOperation::RepositoryDetach {repository:detached})?,
            request(&workspace,FleetOperation::AdrTransition {adr:adr.clone(),transition:AdrTransition::Accept {},expected_revision:SubjectRevision::INITIAL})?,
        ];
        for change in &changes {
            let accepted=changed(&mut session,change).await?;
            let snapshot=session.snapshot().await?;
            if session.write(change).await?!=accepted || session.snapshot().await?!=snapshot{return Err("mutation replay changed its original canonical state or feed head".into());}
        }
        let rejected=request(&workspace,FleetOperation::WorkspaceCreate {name:"Rejected duplicate".to_owned().try_into()?,description:"must keep the first authority".to_owned().try_into()?})?;
        let expected=RequestDecision::Rejected {reasons:vec![Refusal::AlreadyExists(workspace.clone().into())]};
        if session.write(&rejected).await?!=expected{return Err("duplicate workspace did not preserve its exact rejection".into());}
        let after=feed(&mut session).await?;
        if after.1!=43 || after.0.len()!=43 || after.0.get(..before.0.len())!=Some(before.0.as_slice()){return Err("later canonical changes rewrote historical body, list, actor, time, request or order".into());}
        if !matches!(after.0.last(),Some(Record::Request(value)) if value.request_id==rejected.request_id && matches!(&value.decision,message_board::StoredDecision::Rejected {reasons} if reasons==&vec![Refusal::AlreadyExists(workspace.clone().into())])){return Err("feed lost the exact immutable rejected ledger authority".into());}
        if session.write(&rejected).await?!=expected || feed(&mut session).await?!=after{return Err("rejected request replay changed immutable history".into());}
        let current=session.snapshot().await?;
        if !current.0.iter().any(|record|matches!(record,Record::Workspace(value) if value.workspace_id==workspace && value.name.as_str()=="Later canonical workspace" && value.repositories==remaining_repositories))
            || !current.0.iter().any(|record|matches!(record,Record::Adr(value) if value.adr_id==adr && value.status==AdrStatus::Accepted && value.repository_files==files)){
            return Err("canonical mutations did not actually take effect beside the frozen history".into());
        }
        println!("FLEET_FEED_COLUMNS_CASE frozen_complete_prefix_after_canonical_changes; sixteen_ordered_refs_and_files; immutable_accepted_and_rejected_ledgers; exact_replays");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = session.close().await;
    match (proof, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(proof), Err(close)) => Err(format!("feed history: {proof}; close: {close}").into()),
    }
}
