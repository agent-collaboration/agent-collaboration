//! One ordinary session checks ADR body/list fidelity, lifecycle and canonical replay.
use crate::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::{
    AdrId, AdrStatus, AdrTransition, ArtifactRef, ChangeOutcome, ProtocolVersion, Record,
    RecordKind, RecordRef, RecordedActor, RequestDecision, RequestId, SubjectRevision, WorkspaceId,
};

fn request(workspace: &WorkspaceId, operation: FleetOperation) -> ConformanceResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace: workspace.clone(),
        actor: RecordedActor::Human {
            human_id: "typed ADR owner".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}

fn find_adr(records: &[Record], id: &AdrId) -> ConformanceResult<message_board::Adr> {
    records
        .iter()
        .find_map(|record| match record {
            Record::Adr(adr) if &adr.adr_id == id => Some(adr.clone()),
            _ => None,
        })
        .ok_or_else(|| "canonical ADR snapshot omitted the exact identity".into())
}

async fn accepted(
    session: &mut NativeBoardSession,
    request: &FleetRequest,
) -> ConformanceResult<RequestDecision> {
    let decision = session.write(request).await?;
    if !matches!(
        &decision,
        RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        }
    ) {
        return Err(
            format!("ADR operation did not change its canonical record: {decision:?}").into(),
        );
    }
    Ok(decision)
}

pub async fn run_adr_round_trip_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut session = NativeBoardSession(
        enroll_admitted_peer(
            client,
            "019f0000-0000-7000-8000-000000009e20",
            "019f0000-0000-7000-8000-000000009e21",
            "typed-adr-session",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let proof=async {
        let workspace=WorkspaceId::generate();
        accepted(&mut session,&request(&workspace,FleetOperation::WorkspaceCreate {name:"ADR workspace".to_owned().try_into()?,description:"typed canonical ADR owner".to_owned().try_into()?})?).await?;
        let id=AdrId::generate();
        let files=(0..16).map(|ordinal|Ok(ArtifactRef {repository:format!("example.test/adr-file-{ordinal}").try_into()?,path:format!("docs/ADR-\0-{ordinal}.md"),commit:if ordinal%2==0{Some("f".repeat(64).try_into()?)}else{None}})).collect::<ConformanceResult<Vec<_>>>()?;
        let made_in=RecordRef {kind:RecordKind::StandDownAcknowledgement,id:uuid::Uuid::now_v7().to_string().try_into()?};
        let text="# Literal ADR\nPreserve \"quotes\", λ, and ordered artifacts.";
        let create=request(&workspace,FleetOperation::AdrCreate {adr:id.clone(),title:"Literal typed ADR".to_owned().try_into()?,text:text.to_owned().try_into()?,supersedes:None,made_in:Some(made_in.clone()),repository_files:files.clone()})?;
        let decision=accepted(&mut session,&create).await?;
        let before=session.snapshot().await?;
        let stored=find_adr(&before.0,&id)?;
        if stored.text.as_str()!=text || stored.title.as_str()!="Literal typed ADR" || stored.repository_files!=files || stored.made_in!=Some(made_in) || stored.status!=AdrStatus::Proposed || stored.revision!=SubjectRevision::INITIAL || stored.envelope.actor!=create.actor || stored.envelope.request.as_ref()!=Some(&create.request_id) {
            return Err("canonical ADR creation changed supplied body/list/envelope fields".into());
        }
        if session.write(&create).await?!=decision || session.snapshot().await?!=before{return Err("ADR creation replay rewrote canonical state or its feed".into());}
        let transition=request(&workspace,FleetOperation::AdrTransition {adr:id.clone(),transition:AdrTransition::Accept {},expected_revision:SubjectRevision::INITIAL})?;
        let decided=accepted(&mut session,&transition).await?;
        let after=session.snapshot().await?;
        let accepted_adr=find_adr(&after.0,&id)?;
        if accepted_adr.status!=AdrStatus::Accepted || accepted_adr.revision.get()!=1 || accepted_adr.text!=stored.text || accepted_adr.repository_files!=stored.repository_files || accepted_adr.made_in!=stored.made_in || accepted_adr.envelope.request.as_ref()!=Some(&transition.request_id){return Err("ADR transition changed immutable body/list or lost its new envelope".into());}
        if session.write(&transition).await?!=decided || session.snapshot().await?!=after{return Err("ADR transition replay rewrote canonical state or its feed".into());}
        println!("FLEET_ADR_COLUMNS_CASE exact_body_and_sixteen_files; all_reference_fields_preserved; proposed_to_accepted; exact_replays_no_new_feed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = session.close().await;
    match (proof, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(proof), Err(close)) => Err(format!("ADR scenario: {proof}; close: {close}").into()),
    }
}
