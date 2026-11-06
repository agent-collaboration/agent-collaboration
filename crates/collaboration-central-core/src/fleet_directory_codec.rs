//! Reconstruct canonical directory identity and its complete envelope from typed fields.
use crate::{
    fleet_column_codec::{ActorColumns, canonical, optional_text, text, unsigned},
    fleet_record_codec::{
        DecodedFleetRecord, FleetRecordHeader, corrupt, decode_digest, validate_record,
    },
    fleet_row_codec::check_millis,
    generated_fleet_record_reads::StoredFleetProject,
};
use durable_steps::StoreError;
use message_board::{
    BoardId, FleetFeedEntry, FleetPosition, ProjectDirectoryEntry, ProjectId, Record,
    RecordEnvelope, RecordPlacement, RecordScope, RecordTime, WorkspaceId,
};

pub(crate) fn decode(row: StoredFleetProject) -> Result<DecodedFleetRecord, StoreError> {
    let project_id: ProjectId = canonical(
        text(row.project_id, row.project_id_class, "directory.id")?,
        "directory.id",
    )?;
    let board_id: BoardId = canonical(
        text(row.board_id, row.board_id_class, "directory.board")?,
        "directory.board",
    )?;
    let workspace_id: WorkspaceId = canonical(
        text(
            row.workspace_id,
            row.workspace_id_class,
            "directory.workspace",
        )?,
        "directory.workspace",
    )?;
    check_millis(row.created_at, row.created_at_class, "directory.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "directory.updated_at")?;
    let header = FleetRecordHeader::decode(
        optional_text(
            row.router_time,
            row.router_time_class,
            "directory.router_time",
        )?,
        Some(text(
            row.time_basis,
            row.time_basis_class,
            "directory.time_basis",
        )?),
        Some(text(
            row.origin_installation,
            row.origin_installation_class,
            "directory.origin_installation",
        )?),
        Some(text(
            row.origin_machine,
            row.origin_machine_class,
            "directory.origin_machine",
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
    let request = optional_text(row.request_id, row.request_id_class, "directory.request")?
        .map(|value| canonical(value, "directory.request"))
        .transpose()?;
    let position = FleetPosition::try_from(unsigned(
        row.fleet_position,
        row.fleet_position_class,
        "directory.position",
    )?)
    .map_err(|error| corrupt("directory.position", error))?;
    let import_digest = decode_digest(optional_text(
        row.import_digest,
        row.import_digest_class,
        "directory.import_digest",
    )?)?;
    if header.time == RecordTime::LegacyUnknown && import_digest.is_none() {
        return Err(corrupt(
            "directory.import_digest",
            "unknown time requires imported provenance",
        ));
    }
    let record = Record::DirectoryEntry(ProjectDirectoryEntry {
        envelope: RecordEnvelope {
            id: project_id.clone().into(),
            scope: RecordScope {
                workspace: workspace_id,
                board: Some(board_id.clone()),
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
        id: project_id,
        board: board_id,
        title: canonical(
            text(row.name, row.name_class, "directory.name")?,
            "directory.name",
        )?,
    });
    validate_record(&record)?;
    Ok(DecodedFleetRecord {
        record,
        import_digest,
    })
}
