//! Validate Fleet payloads and their physical headers at the SQL boundary.

use durable_steps::StoreError;
use fleet_admission::FleetImportDigest;
use message_board::{
    ActorOrigin, MachineLabel, Record, RecordKind, RecordPlacement, RecordTime, ServiceId,
};
use serde::{Serialize, de::DeserializeOwned};

#[derive(serde::Serialize)]
pub(crate) struct FleetRecordHeader {
    pub time: RecordTime,
    pub origin: ActorOrigin,
}

impl FleetRecordHeader {
    pub fn for_record(record: &Record) -> Result<Self, StoreError> {
        validate_record(record)?;
        let envelope = record.envelope();
        Ok(Self {
            time: envelope.time,
            origin: envelope
                .position
                .origin()
                .cloned()
                .ok_or_else(|| corrupt("origin", "Fleet record has no origin"))?,
        })
    }

    pub fn decode(
        router_time: Option<String>,
        basis: Option<String>,
        installation: Option<String>,
        machine: Option<String>,
    ) -> Result<Self, StoreError> {
        let time = match (required(basis, "time_basis")?.as_str(), router_time) {
            ("legacy_unknown", None) => RecordTime::LegacyUnknown,
            ("router", Some(value)) => {
                let time: chrono::DateTime<chrono::Utc> = value
                    .parse()
                    .map_err(|error| corrupt("router_time", error))?;
                if time.to_rfc3339() != value
                    && time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true) != value
                {
                    return Err(corrupt("router_time", "noncanonical Router timestamp"));
                }
                RecordTime::Router(time)
            }
            _ => return Err(corrupt("time_basis", "timestamp and time basis disagree")),
        };
        let installation = required(installation, "origin_installation")?;
        let machine = required(machine, "origin_machine")?;
        let origin = ActorOrigin {
            installation: ServiceId::try_from(installation.clone())
                .map_err(|error| corrupt("origin_installation", error))?,
            machine: MachineLabel::try_from(machine.clone())
                .map_err(|error| corrupt("origin_machine", error))?,
        };
        if origin.installation.as_str() != installation || origin.machine.as_str() != machine {
            return Err(corrupt("origin", "stored origin would be normalized"));
        }
        Ok(Self { time, origin })
    }

    pub fn router_time(&self) -> Option<String> {
        self.time.as_router_time().map(|time| time.to_rfc3339())
    }

    pub fn time_basis(&self) -> String {
        match self.time {
            RecordTime::Router(_) => "router",
            RecordTime::LegacyUnknown => "legacy_unknown",
        }
        .into()
    }

    pub fn check_record(&self, record: &Record) -> Result<(), StoreError> {
        let expected = Self::for_record(record)?;
        if self.time != expected.time || self.origin != expected.origin {
            return Err(corrupt(
                "headers",
                "record time or origin disagrees with payload",
            ));
        }
        Ok(())
    }
}

pub(crate) struct DecodedFleetRecord {
    pub record: Record,
    pub import_digest: Option<FleetImportDigest>,
}

pub(crate) fn validate_record(record: &Record) -> Result<(), StoreError> {
    record
        .validate_bounds()
        .map_err(|error| corrupt("payload", error))?;
    let envelope = record.envelope();
    if envelope.id != record.reference().id
        || !matches!(envelope.position, RecordPlacement::Fleet(_))
        || envelope.scope.project.is_some()
    {
        return Err(corrupt(
            "envelope",
            "record violates Fleet placement or identity",
        ));
    }
    let coherent = match record {
        Record::Workspace(value) => {
            value.workspace_id == envelope.scope.workspace && envelope.scope.board.is_none()
        }
        Record::Board(value) => {
            value.workspace_id == envelope.scope.workspace
                && envelope.scope.board.as_ref() == Some(&value.board_id)
        }
        Record::DirectoryEntry(value) => envelope.scope.board.as_ref() == Some(&value.board),
        Record::Adr(value) => {
            value.workspace == envelope.scope.workspace && envelope.scope.board.is_none()
        }
        Record::Request(value) => {
            crate::fleet_decision_step::validate_ledger(value)?;
            true
        }
        _ => false,
    };
    if !coherent {
        return Err(corrupt("scope", "record violates Fleet scope"));
    }
    Ok(())
}

pub(crate) fn kind_tag(kind: RecordKind) -> Result<&'static str, StoreError> {
    match kind {
        RecordKind::Workspace => Ok("workspace"),
        RecordKind::Board => Ok("board"),
        RecordKind::Project => Ok("project"),
        RecordKind::Adr => Ok("adr"),
        RecordKind::Request => Ok("request"),
        _ => Err(corrupt("kind", "project-only record requested from Fleet")),
    }
}

pub(crate) fn decode_digest(
    value: Option<String>,
) -> Result<Option<FleetImportDigest>, StoreError> {
    value
        .map(|value| FleetImportDigest::new(value).map_err(|error| corrupt("import_digest", error)))
        .transpose()
}

pub(crate) fn decode_json<T: DeserializeOwned + Serialize>(
    value: &str,
    field: &str,
) -> Result<T, StoreError> {
    let decoded: T = serde_json::from_str(value).map_err(|error| corrupt(field, error))?;
    let original: serde_json::Value =
        serde_json::from_str(value).map_err(|error| corrupt(field, error))?;
    if serde_json::to_value(&decoded).map_err(StoreError::backend)? != original {
        return Err(corrupt(field, "stored value would be normalized"));
    }
    Ok(decoded)
}

pub(crate) use crate::fleet_row_codec::{check_millis, check_private_id, corrupt, required};
