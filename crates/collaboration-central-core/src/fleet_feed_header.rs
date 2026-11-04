//! Validate typed historical envelope fields independently of canonical metadata rows.
use crate::{
    fleet_column_codec::{ActorColumns, canonical, optional_text, text, unsigned},
    fleet_record_codec::FleetRecordHeader,
    fleet_row_codec::{check_millis, check_private_id, corrupt},
    generated_fleet_feed::StoredFleetFeedEntry,
};
use chrono::{DateTime, SecondsFormat, Utc};
use durable_steps::StoreError;
use message_board::{
    FleetFeedEntry as FleetPlacement, FleetPosition, RecordEnvelope, RecordPlacement, RecordScope,
};
pub(crate) struct FeedHeader {
    pub entry_id: String,
    pub position: FleetPosition,
    pub envelope: RecordEnvelope,
    pub as_of: DateTime<Utc>,
}
pub(crate) fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}
pub(crate) fn decode(row: &mut StoredFleetFeedEntry) -> Result<FeedHeader, StoreError> {
    let entry_id = text(
        row.entry_id.take(),
        row.entry_id_class.take(),
        "feed.entry_id",
    )?;
    check_private_id(&entry_id, "feed.entry_id")?;
    let position = FleetPosition::try_from(unsigned(
        row.position.take(),
        row.position_class.take(),
        "feed.position",
    )?)
    .map_err(|error| corrupt("feed.position", error))?;
    let as_of_text = text(row.as_of.take(), row.as_of_class.take(), "feed.as_of")?;
    let as_of: DateTime<Utc> = as_of_text
        .parse()
        .map_err(|error| corrupt("feed.as_of", error))?;
    if timestamp(as_of) != as_of_text {
        return Err(corrupt("feed.as_of", "noncanonical retention timestamp"));
    }
    check_millis(
        row.created_at.take(),
        row.created_at_class.take(),
        "feed.created_at",
    )?;
    check_millis(
        row.updated_at.take(),
        row.updated_at_class.take(),
        "feed.updated_at",
    )?;
    let header = FleetRecordHeader::decode(
        optional_text(
            row.router_time.take(),
            row.router_time_class.take(),
            "feed.router_time",
        )?,
        Some(text(
            row.time_basis.take(),
            row.time_basis_class.take(),
            "feed.time_basis",
        )?),
        Some(text(
            row.origin_installation.take(),
            row.origin_installation_class.take(),
            "feed.origin_installation",
        )?),
        Some(text(
            row.origin_machine.take(),
            row.origin_machine_class.take(),
            "feed.origin_machine",
        )?),
    )?;
    let actor = ActorColumns {
        kind: text(
            row.actor_kind.take(),
            row.actor_kind_class.take(),
            "feed.actor_kind",
        )?,
        human: optional_text(
            row.actor_human_id.take(),
            row.actor_human_id_class.take(),
            "feed.actor_human_id",
        )?,
        service: optional_text(
            row.actor_service_id.take(),
            row.actor_service_id_class.take(),
            "feed.actor_service_id",
        )?,
        endpoint: optional_text(
            row.actor_endpoint_id.take(),
            row.actor_endpoint_id_class.take(),
            "feed.actor_endpoint_id",
        )?,
        session: optional_text(
            row.actor_session_id.take(),
            row.actor_session_id_class.take(),
            "feed.actor_session_id",
        )?,
        component: optional_text(
            row.actor_router_component.take(),
            row.actor_router_component_class.take(),
            "feed.actor_router_component",
        )?,
    }
    .decode()?;
    let envelope = RecordEnvelope {
        id: canonical(
            text(
                row.record_id.take(),
                row.record_id_class.take(),
                "feed.record_id",
            )?,
            "feed.record_id",
        )?,
        scope: RecordScope {
            workspace: canonical(
                text(
                    row.workspace_id.take(),
                    row.workspace_id_class.take(),
                    "feed.workspace",
                )?,
                "feed.workspace",
            )?,
            board: optional_text(row.board_id.take(), row.board_id_class.take(), "feed.board")?
                .map(|value| canonical(value, "feed.board"))
                .transpose()?,
            project: None,
        },
        actor,
        request: optional_text(
            row.envelope_request_id.take(),
            row.envelope_request_id_class.take(),
            "feed.request",
        )?
        .map(|value| canonical(value, "feed.request"))
        .transpose()?,
        position: RecordPlacement::Fleet(FleetPlacement {
            position,
            origin: header.origin,
        }),
        time: header.time,
    };
    Ok(FeedHeader {
        entry_id,
        position,
        envelope,
        as_of,
    })
}
