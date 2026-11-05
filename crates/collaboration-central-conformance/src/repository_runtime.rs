//! Ordinary Fleet frames keep typed repository associations and canonical list mutations aligned.
use crate::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
    native_board_session::NativeBoardSession,
};
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::{
    ChangeOutcome, ProtocolVersion, Record, RecordedActor, RepositoryRef, RequestDecision,
    RequestId, WorkspaceId,
};

fn request(workspace: &WorkspaceId, operation: FleetOperation) -> ConformanceResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace: workspace.clone(),
        actor: RecordedActor::Human {
            human_id: "repository owner".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
async fn repositories(
    session: &mut NativeBoardSession,
    workspace: &WorkspaceId,
) -> ConformanceResult<Vec<RepositoryRef>> {
    session
        .snapshot()
        .await?
        .0
        .into_iter()
        .find_map(|record| match record {
            Record::Workspace(value) if &value.workspace_id == workspace => {
                Some(value.repositories)
            }
            _ => None,
        })
        .ok_or_else(|| "canonical repository scenario workspace absent".into())
}
async fn changed(
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
        return Err(format!("repository mutation was not committed: {decision:?}").into());
    }
    Ok(decision)
}

pub async fn run_repository_round_trip_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut session = NativeBoardSession(
        enroll_admitted_peer(
            client,
            "019f0000-0000-7000-8000-000000009e40",
            "019f0000-0000-7000-8000-000000009e41",
            "typed-repository-session",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?,
    );
    let proof = async {
        let workspace = WorkspaceId::generate();
        changed(
            &mut session,
            &request(&workspace, FleetOperation::WorkspaceCreate {
                name: "Repository workspace".to_owned().try_into()?,
                description: "separate list and association owners".to_owned().try_into()?,
            })?,
        ).await?;
        let origin = RepositoryRef::Origin {
            normalized_origin: "example.test/literal-repository".to_owned().try_into()?,
        };
        let first = RepositoryRef::Local {
            service_id: "019f0000-0000-7000-8000-000000009e42".to_owned().try_into()?,
            common_directory: "/repository/shared/λ".to_owned().try_into()?,
        };
        let second = RepositoryRef::Local {
            service_id: "019f0000-0000-7000-8000-000000009e43".to_owned().try_into()?,
            common_directory: "/repository/shared/λ".to_owned().try_into()?,
        };
        let expected = vec![origin.clone(), first.clone(), second.clone()];
        for reference in [&second, &origin, &first] {
            let offered = request(&workspace, FleetOperation::RepositoryAttach {
                repository: reference.clone(),
            })?;
            let accepted = changed(&mut session, &offered).await?;
            let snapshot = session.snapshot().await?;
            if session.write(&offered).await? != accepted || session.snapshot().await? != snapshot {
                return Err("attachment replay changed canonical state or appended feed".into());
            }
        }
        if repositories(&mut session, &workspace).await? != expected {
            return Err("typed origin/local keys collapsed distinct services or changed canonical list order".into());
        }
        let duplicate = request(&workspace, FleetOperation::RepositoryAttach {
            repository: first.clone(),
        })?;
        if !matches!(session.write(&duplicate).await?, RequestDecision::Accepted {
            change: ChangeOutcome::AlreadySo, ..
        }) {
            return Err("a fresh duplicate attachment was not already-so".into());
        }
        if repositories(&mut session, &workspace).await? != expected {
            return Err("duplicate attachment rewrote the canonical list".into());
        }
        let detach = request(&workspace, FleetOperation::RepositoryDetach {repository: first})?;
        let accepted = changed(&mut session, &detach).await?;
        if repositories(&mut session, &workspace).await? != vec![origin, second] {
            return Err("detaching one local key removed another service at the same directory".into());
        }
        let snapshot = session.snapshot().await?;
        if session.write(&detach).await? != accepted || session.snapshot().await? != snapshot {
            return Err("detach replay changed canonical state or feed".into());
        }
        println!("FLEET_REPOSITORY_COLUMNS_CASE origin_and_distinct_local_keys; exact_attach_and_detach_replays; canonical_list_order; duplicate_already_so");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = session.close().await;
    match (proof, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(proof), Err(close)) => {
            Err(format!("repository scenario: {proof}; close: {close}").into())
        }
    }
}
