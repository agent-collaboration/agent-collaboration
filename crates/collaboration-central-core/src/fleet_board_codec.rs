//! Reconstruct canonical Board variants and their complete envelope from typed fields.
use crate::{
    fleet_column_codec::{ActorColumns, canonical, optional_text, text, unsigned},
    fleet_record_codec::{
        DecodedFleetRecord, FleetRecordHeader, corrupt, decode_digest, validate_record,
    },
    fleet_row_codec::check_millis,
    generated_fleet_boards::StoredFleetBoard,
};
use durable_steps::StoreError;
use message_board::{
    Board, BoardId, BoardKind, BoardState, FleetFeedEntry, FleetPosition, Record, RecordEnvelope,
    RecordPlacement, RecordScope, RecordTime, WorkspaceId,
};

pub(crate) fn decode(row: StoredFleetBoard) -> Result<DecodedFleetRecord, StoreError> {
    let board_id: BoardId = canonical(
        text(row.board_id, row.board_id_class, "board.id")?,
        "board.id",
    )?;
    let workspace_id: WorkspaceId = canonical(
        text(row.workspace_id, row.workspace_id_class, "board.workspace")?,
        "board.workspace",
    )?;
    check_millis(row.created_at, row.created_at_class, "board.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "board.updated_at")?;
    let header = FleetRecordHeader::decode(
        optional_text(row.router_time, row.router_time_class, "board.router_time")?,
        Some(text(
            row.time_basis,
            row.time_basis_class,
            "board.time_basis",
        )?),
        Some(text(
            row.origin_installation,
            row.origin_installation_class,
            "board.origin_installation",
        )?),
        Some(text(
            row.origin_machine,
            row.origin_machine_class,
            "board.origin_machine",
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
    let request = optional_text(row.request_id, row.request_id_class, "board.request")?
        .map(|value| canonical(value, "board.request"))
        .transpose()?;
    let position = FleetPosition::try_from(unsigned(
        row.fleet_position,
        row.fleet_position_class,
        "board.position",
    )?)
    .map_err(|error| corrupt("board.position", error))?;
    let import_digest = decode_digest(optional_text(
        row.import_digest,
        row.import_digest_class,
        "board.import_digest",
    )?)?;
    if header.time == RecordTime::LegacyUnknown && import_digest.is_none() {
        return Err(corrupt(
            "board.import_digest",
            "unknown time requires imported provenance",
        ));
    }
    let goal = optional_text(row.initiative_goal, row.initiative_goal_class, "board.goal")?;
    let done_when = optional_text(
        row.initiative_done_when,
        row.initiative_done_when_class,
        "board.done_when",
    )?;
    let kind = match (
        text(row.board_kind, row.board_kind_class, "board.kind")?.as_str(),
        goal,
        done_when,
    ) {
        ("area", None, None) => BoardKind::Area {},
        ("initiative", Some(goal), Some(done_when)) => BoardKind::Initiative { goal, done_when },
        _ => {
            return Err(corrupt(
                "board.kind",
                "unknown kind or fields outside its active variant",
            ));
        }
    };
    let record = Record::Board(Board {
        envelope: RecordEnvelope {
            id: board_id.clone().into(),
            scope: RecordScope {
                workspace: workspace_id.clone(),
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
        board_id,
        workspace_id,
        name: canonical(text(row.name, row.name_class, "board.name")?, "board.name")?,
        description: canonical(
            text(row.description, row.description_class, "board.description")?,
            "board.description",
        )?,
        state: match text(row.state, row.state_class, "board.state")?.as_str() {
            "open" => BoardState::Open,
            "closed" => BoardState::Closed,
            "archiving" => BoardState::Archiving,
            "archived" => BoardState::Archived,
            _ => return Err(corrupt("board.state", "unknown board state")),
        },
        kind,
    });
    validate_record(&record)?;
    Ok(DecodedFleetRecord {
        record,
        import_digest,
    })
}
