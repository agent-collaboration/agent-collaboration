//! Decode immutable ADR text, lifecycle and ordered repository files from typed rows.
use crate::{
    fleet_column_codec::{ActorColumns, canonical, optional_text, text, unsigned},
    fleet_record_codec::{
        DecodedFleetRecord, FleetRecordHeader, corrupt, decode_digest, validate_record,
    },
    fleet_row_codec::{check_millis, check_private_id},
    generated_fleet_adrs::{StoredAdrRepositoryFile, StoredFleetAdr},
};
use durable_steps::StoreError;
use message_board::{
    Adr, AdrId, AdrStatus, ArtifactRef, FleetFeedEntry, FleetPosition, Record, RecordEnvelope,
    RecordKind, RecordPlacement, RecordRef, RecordScope, RecordTime, SubjectRevision, WorkspaceId,
};

pub(crate) fn decode(
    row: StoredFleetAdr,
    children: Vec<StoredAdrRepositoryFile>,
) -> Result<DecodedFleetRecord, StoreError> {
    let adr_id: AdrId = canonical(text(row.adr_id, row.adr_id_class, "adr.id")?, "adr.id")?;
    let workspace_id: WorkspaceId = canonical(
        text(row.workspace_id, row.workspace_id_class, "adr.workspace")?,
        "adr.workspace",
    )?;
    let count = unsigned(
        row.repository_file_count,
        row.repository_file_count_class,
        "adr.repository_file_count",
    )?;
    if count > 16 || usize::try_from(count).ok() != Some(children.len()) {
        return Err(corrupt(
            "adr.repository_files",
            "child rows do not match the bounded parent count",
        ));
    }
    check_millis(row.created_at, row.created_at_class, "adr.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "adr.updated_at")?;
    let header = FleetRecordHeader::decode(
        optional_text(row.router_time, row.router_time_class, "adr.router_time")?,
        Some(text(
            row.time_basis,
            row.time_basis_class,
            "adr.time_basis",
        )?),
        Some(text(
            row.origin_installation,
            row.origin_installation_class,
            "adr.origin_installation",
        )?),
        Some(text(
            row.origin_machine,
            row.origin_machine_class,
            "adr.origin_machine",
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
    let request = optional_text(row.request_id, row.request_id_class, "adr.request")?
        .map(|value| canonical(value, "adr.request"))
        .transpose()?;
    let position = FleetPosition::try_from(unsigned(
        row.fleet_position,
        row.fleet_position_class,
        "adr.position",
    )?)
    .map_err(|error| corrupt("adr.position", error))?;
    let import_digest = decode_digest(optional_text(
        row.import_digest,
        row.import_digest_class,
        "adr.import_digest",
    )?)?;
    if header.time == RecordTime::LegacyUnknown && import_digest.is_none() {
        return Err(corrupt(
            "adr.import_digest",
            "unknown time requires imported provenance",
        ));
    }
    let repository_files = children
        .into_iter()
        .enumerate()
        .map(|(index, child)| decode_file(child, &adr_id, index))
        .collect::<Result<Vec<_>, _>>()?;
    let supersedes = optional_text(
        row.supersedes_adr_id,
        row.supersedes_adr_id_class,
        "adr.supersedes",
    )?
    .map(|value| canonical(value, "adr.supersedes"))
    .transpose()?;
    let made_in = match (
        optional_text(row.made_in_kind, row.made_in_kind_class, "adr.made_in_kind")?,
        optional_text(
            row.made_in_record_id,
            row.made_in_record_id_class,
            "adr.made_in_record_id",
        )?,
    ) {
        (None, None) => None,
        (Some(kind), Some(id)) => Some(RecordRef {
            kind: decode_record_kind(kind)?,
            id: canonical(id, "adr.made_in_record_id")?,
        }),
        _ => {
            return Err(corrupt(
                "adr.made_in",
                "kind and identity must occur together",
            ));
        }
    };
    let revision_text = text(row.revision, row.revision_class, "adr.revision")?;
    let revision_value = revision_text
        .parse::<u64>()
        .map_err(|error| corrupt("adr.revision", error))?;
    if revision_value.to_string() != revision_text {
        return Err(corrupt("adr.revision", "noncanonical revision"));
    }
    let revision = SubjectRevision::try_from(revision_value)
        .map_err(|error| corrupt("adr.revision", error))?;
    let record = Record::Adr(Adr {
        envelope: RecordEnvelope {
            id: adr_id.clone().into(),
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
        adr_id,
        workspace: workspace_id,
        title: canonical(text(row.title, row.title_class, "adr.title")?, "adr.title")?,
        text: canonical(text(row.text, row.text_class, "adr.text")?, "adr.text")?,
        status: match text(row.status, row.status_class, "adr.status")?.as_str() {
            "proposed" => AdrStatus::Proposed,
            "accepted" => AdrStatus::Accepted,
            "superseded" => AdrStatus::Superseded,
            "withdrawn" => AdrStatus::Withdrawn,
            _ => return Err(corrupt("adr.status", "unknown ADR status")),
        },
        supersedes,
        made_in,
        revision,
        repository_files,
    });
    validate_record(&record)?;
    Ok(DecodedFleetRecord {
        record,
        import_digest,
    })
}

fn decode_record_kind(value: String) -> Result<RecordKind, StoreError> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|error| corrupt("adr.made_in_kind", error))
}

fn decode_file(
    row: StoredAdrRepositoryFile,
    adr: &AdrId,
    index: usize,
) -> Result<ArtifactRef, StoreError> {
    check_private_id(&text(row.id, row.id_class, "adr.file.id")?, "adr.file.id")?;
    if text(row.adr_id, row.adr_id_class, "adr.file.parent")? != adr.as_str()
        || usize::try_from(unsigned(
            row.ordinal,
            row.ordinal_class,
            "adr.file.ordinal",
        )?)
        .ok()
            != Some(index)
    {
        return Err(corrupt(
            "adr.files",
            "wrong parent or a noncontiguous ordinal",
        ));
    }
    check_millis(row.created_at, row.created_at_class, "adr.file.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "adr.file.updated_at")?;
    Ok(ArtifactRef {
        repository: canonical(
            text(
                row.repository_origin,
                row.repository_origin_class,
                "adr.file.repository",
            )?,
            "adr.file.repository",
        )?,
        path: text(row.path, row.path_class, "adr.file.path")?,
        commit: optional_text(row.commit_sha, row.commit_sha_class, "adr.file.commit")?
            .map(|value| canonical(value, "adr.file.commit"))
            .transpose()?,
    })
}
