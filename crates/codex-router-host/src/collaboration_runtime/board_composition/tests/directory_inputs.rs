//! Typed directory inputs for native Host component proofs; central policy is separate.
use super::*;

pub(super) async fn arrange_project(
    board: &BoardComposition,
    root: &Path,
) -> Result<(Arc<FleetClient>, ProjectCreateRequest), Box<dyn std::error::Error + Send + Sync>> {
    let (fleet, request) = arrange_project_input(board, root).await?;
    let reply = board.admission.create_project(&request).await?;
    assert_eq!(reply.result, RequestResult::PendingUnreachable);
    Ok((fleet, request))
}

pub(super) async fn arrange_project_input(
    board: &BoardComposition,
    root: &Path,
) -> Result<(Arc<FleetClient>, ProjectCreateRequest), Box<dyn std::error::Error + Send + Sync>> {
    let fleet = board.fleet.upgrade().ok_or("fleet client closed")?;
    let workspace = WorkspaceId::generate();
    let board_id = BoardId::generate();
    let actor = Identity::Human {
        human_id: HumanId::try_from("session owner".to_owned())?,
    };
    // This component fixture supplies directory policy, not a central-policy RPC.
    // Seed through the validated cache port; the generic FleetService has no
    // board-policy implementation. Project creation still enters real admission.
    let record_origin = origin()?;
    let envelope = |id, scope_board, position| RecordEnvelope {
        id,
        scope: RecordScope {
            workspace: workspace.clone(),
            board: scope_board,
            project: None,
        },
        actor: RecordedActor::declared(actor.clone()),
        request: None,
        position: RecordPlacement::Fleet(FleetFeedEntry {
            position,
            origin: record_origin.clone(),
        }),
        time: RecordTime::Router(chrono::Utc::now()),
    };
    let cache = message_board_storage::FleetCache::open(&root.join("fleet-cache.sqlite")).await?;
    cache
        .replace_snapshot(&message_board_storage::FleetRecordSnapshot {
            through: FleetPosition::try_from(2)?,
            records: vec![
                Record::Workspace(Workspace {
                    envelope: envelope(workspace.clone().into(), None, FleetPosition::try_from(1)?),
                    workspace_id: workspace.clone(),
                    name: ResourceName::try_from("session workspace".to_owned())?,
                    description: Description::try_from(String::new())?,
                    state: WorkspaceState::Active,
                    repositories: Vec::new(),
                }),
                Record::Board(Board {
                    envelope: envelope(
                        board_id.clone().into(),
                        Some(board_id.clone()),
                        FleetPosition::try_from(2)?,
                    ),
                    board_id: board_id.clone(),
                    workspace_id: workspace,
                    name: ResourceName::try_from("session board".to_owned())?,
                    description: Description::try_from(String::new())?,
                    kind: BoardKind::Area {},
                    state: BoardState::Open,
                }),
            ],
        })
        .await?;
    cache.close().await;
    let request = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: ProjectId::generate(),
        board_id,
        title: ResourceName::try_from("session project".to_owned())?,
        description: Some(Description::try_from(
            "first registered description".to_owned(),
        )?),
        actor,
        acting_for: None,
    };
    Ok((fleet, request))
}

pub(super) async fn settle_fixture_directory(
    fleet: &FleetClient,
    request: &ProjectCreateRequest,
) -> Result<Option<ProjectRegistrationState>, Box<dyn std::error::Error + Send + Sync>> {
    assert!(fleet.pending_request(&request.request_id).await?.is_some());
    assert_eq!(
        fleet.creation_input(&request.request_id).await?,
        Some(request.clone()),
        "Host admission did not durably queue the complete creating input"
    );
    // A typed Accepted directory input is the boundary of these component cases.
    // Policy writes are not answered by this fixture. Native registration and
    // Copy use the real Host/session/runtime and decide whether creation may write.
    let effects = vec![(
        request.project_id.clone().into(),
        RecordPlacement::Fleet(FleetFeedEntry {
            position: FleetPosition::try_from(3)?,
            origin: origin()?,
        }),
    )];
    let decision = RequestDecision::Accepted {
        effects: DecisionEffectsPage::from_full(
            effects.clone(),
            &DecisionEffectsHeader::from_full(&effects),
            None,
        )?,
        change: ChangeOutcome::Changed,
    };
    Ok(fleet
        .settle_central_reply(&request.request_id, &decision)
        .await?)
}
