//! Reconstruct canonical Workspaces from typed parent and ordered child rows.
use crate::{
    fleet_column_codec::{ActorColumns, canonical, optional_text, text, unsigned},
    fleet_record_codec::{
        DecodedFleetRecord, FleetRecordHeader, corrupt, decode_digest, validate_record,
    },
    fleet_row_codec::{check_millis, check_private_id},
    generated_fleet_workspaces::{StoredFleetWorkspace, StoredWorkspaceRepositoryRef},
};
use durable_steps::StoreError;
use message_board::{
    FleetFeedEntry, FleetPosition, Record, RecordEnvelope, RecordPlacement, RecordScope,
    RecordTime, RepositoryRef, Workspace, WorkspaceId, WorkspaceState,
};

pub(crate) fn decode(
    row: StoredFleetWorkspace,
    children: Vec<StoredWorkspaceRepositoryRef>,
) -> Result<DecodedFleetRecord, StoreError> {
    let workspace_id: WorkspaceId = canonical(
        text(row.workspace_id, row.workspace_id_class, "workspace.id")?,
        "workspace.id",
    )?;
    let count = unsigned(
        row.repository_count,
        row.repository_count_class,
        "workspace.repository_count",
    )?;
    if count > 16 || usize::try_from(count).ok() != Some(children.len()) {
        return Err(corrupt(
            "workspace.repositories",
            "child rows do not match the bounded parent count",
        ));
    }
    check_millis(row.created_at, row.created_at_class, "workspace.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "workspace.updated_at")?;
    let header = FleetRecordHeader::decode(
        optional_text(
            row.router_time,
            row.router_time_class,
            "workspace.router_time",
        )?,
        Some(text(
            row.time_basis,
            row.time_basis_class,
            "workspace.time_basis",
        )?),
        Some(text(
            row.origin_installation,
            row.origin_installation_class,
            "workspace.origin_installation",
        )?),
        Some(text(
            row.origin_machine,
            row.origin_machine_class,
            "workspace.origin_machine",
        )?),
    )?;
    let actor = ActorColumns {
        kind: text(row.actor_kind, row.actor_kind_class, "actor.kind")?,
        human: optional_text(row.actor_human_id, row.actor_human_id_class, "actor.human")?,
        service: optional_text(
            row.actor_service_id,
            row.actor_service_id_class,
            "actor.service",
        )?,
        endpoint: optional_text(
            row.actor_endpoint_id,
            row.actor_endpoint_id_class,
            "actor.endpoint",
        )?,
        session: optional_text(
            row.actor_session_id,
            row.actor_session_id_class,
            "actor.session",
        )?,
        component: optional_text(
            row.actor_router_component,
            row.actor_router_component_class,
            "actor.component",
        )?,
    }
    .decode()?;
    let request = optional_text(row.request_id, row.request_id_class, "workspace.request")?
        .map(|value| canonical(value, "workspace.request"))
        .transpose()?;
    let position = FleetPosition::try_from(unsigned(
        row.fleet_position,
        row.fleet_position_class,
        "workspace.position",
    )?)
    .map_err(|error| corrupt("workspace.position", error))?;
    let import_digest = decode_digest(optional_text(
        row.import_digest,
        row.import_digest_class,
        "workspace.import_digest",
    )?)?;
    if header.time == RecordTime::LegacyUnknown && import_digest.is_none() {
        return Err(corrupt(
            "workspace.import_digest",
            "unknown time requires imported provenance",
        ));
    }
    let repositories = children
        .into_iter()
        .enumerate()
        .map(|(index, child)| decode_repository(child, &workspace_id, index))
        .collect::<Result<Vec<_>, _>>()?;
    let record = Record::Workspace(Workspace {
        envelope: RecordEnvelope {
            id: workspace_id.clone().into(),
            scope: RecordScope {
                workspace: workspace_id.clone(),
                board: None,
                project: None,
            },
            actor,
            request,
            position: RecordPlacement::Fleet(FleetFeedEntry {
                position,
                origin: header.origin,
            }),
            time: header.time,
        },
        workspace_id,
        name: canonical(
            text(row.name, row.name_class, "workspace.name")?,
            "workspace.name",
        )?,
        description: canonical(
            text(
                row.description,
                row.description_class,
                "workspace.description",
            )?,
            "workspace.description",
        )?,
        state: match text(row.state, row.state_class, "workspace.state")?.as_str() {
            "active" => WorkspaceState::Active,
            "archiving" => WorkspaceState::Archiving,
            "archived" => WorkspaceState::Archived,
            _ => return Err(corrupt("workspace.state", "unknown workspace state")),
        },
        repositories,
    });
    validate_record(&record)?;
    Ok(DecodedFleetRecord {
        record,
        import_digest,
    })
}

fn decode_repository(
    row: StoredWorkspaceRepositoryRef,
    workspace: &WorkspaceId,
    index: usize,
) -> Result<RepositoryRef, StoreError> {
    check_private_id(
        &text(row.row_id, row.row_id_class, "workspace.repository.id")?,
        "workspace.repository.id",
    )?;
    if text(
        row.workspace_id,
        row.workspace_id_class,
        "workspace.repository.parent",
    )? != workspace.as_str()
        || usize::try_from(unsigned(
            row.ordinal,
            row.ordinal_class,
            "workspace.repository.ordinal",
        )?)
        .ok()
            != Some(index)
    {
        return Err(corrupt(
            "workspace.repositories",
            "wrong parent or a noncontiguous ordinal",
        ));
    }
    check_millis(
        row.created_at,
        row.created_at_class,
        "workspace.repository.created_at",
    )?;
    check_millis(
        row.updated_at,
        row.updated_at_class,
        "workspace.repository.updated_at",
    )?;
    let origin = optional_text(
        row.normalized_origin,
        row.normalized_origin_class,
        "workspace.repository.origin",
    )?;
    let service = optional_text(
        row.service_id,
        row.service_id_class,
        "workspace.repository.service",
    )?;
    let directory = optional_text(
        row.common_directory,
        row.common_directory_class,
        "workspace.repository.directory",
    )?;
    match (
        text(
            row.reference_kind,
            row.reference_kind_class,
            "workspace.repository.kind",
        )?
        .as_str(),
        origin,
        service,
        directory,
    ) {
        ("origin", Some(origin), None, None) => Ok(RepositoryRef::Origin {
            normalized_origin: canonical(origin, "workspace.repository.origin")?,
        }),
        ("local", None, Some(service), Some(directory)) => Ok(RepositoryRef::Local {
            service_id: canonical(service, "workspace.repository.service")?,
            common_directory: canonical(directory, "workspace.repository.directory")?,
        }),
        _ => Err(corrupt(
            "workspace.repository",
            "unknown kind or fields outside its active variant",
        )),
    }
}
