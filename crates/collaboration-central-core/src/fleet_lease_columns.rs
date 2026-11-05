//! Flatten and validate committed Registry snapshots without changing their authority or digest.
use crate::{
    LeaseContentDigest, RegistryIncarnation,
    fleet_column_codec::{canonical, decimal, optional_millis, optional_text, text},
    fleet_row_codec::{corrupt, required},
    generated_fleet_lease::StoredDirectoryLease,
    lease_projection::LeaseProjection,
};
use durable_steps::StoreError;
use fleet_sync_lease::{
    DatabaseLifecycle, ImportOrigin, RegistryGrant, RegistryRevision, RegistrySnapshot,
};
use fleet_sync_protocol::{
    CommitSequence, DatabaseId, Epoch, Origin, ReplicationPosition, SyncTime,
};
use std::num::NonZeroU64;

pub(crate) struct LeaseColumns {
    pub epoch: String,
    pub change: String,
    pub incarnation: String,
    pub changed_at: String,
    pub lifecycle_kind: String,
    pub origin_kind: Option<String>,
    pub origin_epoch: Option<String>,
    pub origin_sequence: Option<String>,
    pub import_digest: Option<String>,
    pub retiring_attempt_id: Option<String>,
    pub retired_at: Option<String>,
    pub holder_machine_id: Option<String>,
    pub holder_service_id: Option<String>,
    pub expires_at: Option<String>,
    pub digest: String,
}
impl LeaseColumns {
    pub(crate) fn encode(projection: &LeaseProjection) -> Self {
        let snapshot = projection.snapshot();
        let (_, incarnation, _) = projection.version().components();
        let mut columns = Self {
            epoch: snapshot.revision.epoch.value().to_string(),
            change: snapshot.revision.change.get().to_string(),
            incarnation: incarnation.value().to_string(),
            changed_at: snapshot.changed_at.unix_millis().to_string(),
            lifecycle_kind: String::new(),
            origin_kind: None,
            origin_epoch: None,
            origin_sequence: None,
            import_digest: None,
            retiring_attempt_id: None,
            retired_at: None,
            holder_machine_id: snapshot
                .grant
                .as_ref()
                .map(|grant| grant.holder.to_string()),
            holder_service_id: projection.holder().map(|holder| holder.as_str().into()),
            expires_at: snapshot
                .grant
                .as_ref()
                .map(|grant| grant.expires_at.unix_millis().to_string()),
            digest: String::from(projection.digest().as_str()),
        };
        columns.lifecycle_kind = match &snapshot.lifecycle {
            DatabaseLifecycle::Registered {
                origin,
                imported_from,
            } => {
                columns.import_digest = imported_from
                    .as_ref()
                    .map(|imported| imported.digest.as_str().to_owned());
                columns.origin_kind = Some(
                    match origin {
                        Origin::Empty => "empty",
                        Origin::Seeded { position } => {
                            columns.origin_epoch = Some(position.epoch.value().to_string());
                            columns.origin_sequence = Some(position.sequence.value().to_string());
                            "seeded"
                        }
                    }
                    .into(),
                );
                "registered"
            }
            DatabaseLifecycle::Unheld { import_digest } => {
                columns.import_digest = Some(import_digest.as_str().to_owned());
                "unheld"
            }
            DatabaseLifecycle::Retiring { attempt } => {
                columns.retiring_attempt_id = Some(attempt.to_string());
                "retiring"
            }
            DatabaseLifecycle::Retired { at } => {
                columns.retired_at = Some(at.unix_millis().to_string());
                "retired"
            }
        }
        .into();
        columns
    }
}

pub(crate) fn decode(row: StoredDirectoryLease) -> Result<Option<LeaseProjection>, StoreError> {
    let database: DatabaseId = canonical(
        text(row.project_id, row.project_id_class, "lease.project")?,
        "lease.project",
    )?;
    let epoch = optional_text(row.lease_epoch, row.lease_epoch_class, "lease.epoch")?;
    let change = optional_text(row.lease_change, row.lease_change_class, "lease.change")?;
    let incarnation = optional_text(
        row.lease_incarnation,
        row.lease_incarnation_class,
        "lease.incarnation",
    )?;
    let changed_at = optional_millis(
        row.lease_changed_at,
        row.lease_changed_at_class,
        "lease.changed_at",
    )?;
    let kind = optional_text(
        row.lease_lifecycle_kind,
        row.lease_lifecycle_kind_class,
        "lease.lifecycle",
    )?;
    let origin = optional_text(
        row.lease_origin_kind,
        row.lease_origin_kind_class,
        "lease.origin",
    )?;
    let origin_epoch = optional_text(
        row.lease_origin_epoch,
        row.lease_origin_epoch_class,
        "lease.origin_epoch",
    )?;
    let origin_sequence = optional_text(
        row.lease_origin_sequence,
        row.lease_origin_sequence_class,
        "lease.origin_sequence",
    )?;
    let imported = optional_text(
        row.lease_import_digest,
        row.lease_import_digest_class,
        "lease.import_digest",
    )?;
    let attempt = optional_text(
        row.lease_retiring_attempt_id,
        row.lease_retiring_attempt_id_class,
        "lease.retiring_attempt",
    )?;
    let retired = optional_millis(
        row.lease_retired_at,
        row.lease_retired_at_class,
        "lease.retired_at",
    )?;
    let machine = optional_text(
        row.lease_holder_machine_id,
        row.lease_holder_machine_id_class,
        "lease.holder_machine",
    )?;
    let service = optional_text(
        row.lease_holder_service_id,
        row.lease_holder_service_id_class,
        "lease.holder_service",
    )?;
    let expires = optional_millis(
        row.lease_expires_at,
        row.lease_expires_at_class,
        "lease.expires_at",
    )?;
    let digest = optional_text(row.lease_digest, row.lease_digest_class, "lease.digest")?;
    if [
        &epoch,
        &change,
        &incarnation,
        &kind,
        &origin,
        &origin_epoch,
        &origin_sequence,
        &imported,
        &attempt,
        &machine,
        &service,
        &digest,
    ]
    .iter()
    .all(|value| value.is_none())
        && changed_at.is_none()
        && retired.is_none()
        && expires.is_none()
    {
        return Ok(None);
    }
    let lifecycle = match (
        required(kind, "lease.lifecycle")?.as_str(),
        origin.as_deref(),
        origin_epoch,
        origin_sequence,
        imported,
        attempt,
        retired,
    ) {
        ("registered", Some(origin), origin_epoch, origin_sequence, imported, None, None) => {
            let origin = match (origin, origin_epoch, origin_sequence) {
                ("empty", None, None) => Origin::Empty,
                ("seeded", Some(epoch), Some(sequence)) => Origin::Seeded {
                    position: ReplicationPosition::new(
                        parse_epoch(&epoch)?,
                        CommitSequence::new(decimal(&sequence, "lease.origin_sequence")?),
                    ),
                },
                _ => return Err(corrupt("lease.origin", "unknown origin or inactive fields")),
            };
            DatabaseLifecycle::Registered {
                origin,
                imported_from: imported
                    .map(|digest| {
                        fleet_sync_protocol::ImportDigest::new(digest)
                            .map_err(|error| corrupt("lease.import_digest", error))
                            .map(|digest| ImportOrigin { digest })
                    })
                    .transpose()?,
            }
        }
        ("unheld", None, None, None, Some(digest), None, None) => DatabaseLifecycle::Unheld {
            import_digest: fleet_sync_protocol::ImportDigest::new(digest)
                .map_err(|error| corrupt("lease.import_digest", error))?,
        },
        ("retiring", None, None, None, None, Some(attempt), None) => DatabaseLifecycle::Retiring {
            attempt: canonical(attempt, "lease.retiring_attempt")?,
        },
        ("retired", None, None, None, None, None, Some(at)) => DatabaseLifecycle::Retired {
            at: SyncTime::from_unix_millis(at),
        },
        _ => {
            return Err(corrupt(
                "lease.lifecycle",
                "unknown lifecycle or inactive fields",
            ));
        }
    };
    let epoch = parse_epoch(&required(epoch, "lease.epoch")?)?;
    let (grant, holder) = match (
        matches!(lifecycle, DatabaseLifecycle::Registered { .. }),
        machine,
        service,
        expires,
    ) {
        (true, Some(machine), Some(service), Some(expires)) => (
            Some(RegistryGrant {
                epoch,
                holder: canonical(machine, "lease.holder_machine")?,
                expires_at: SyncTime::from_unix_millis(expires),
            }),
            Some(canonical(service, "lease.holder_service")?),
        ),
        (false, None, None, None) => (None, None),
        _ => {
            return Err(corrupt(
                "lease.grant",
                "partial grant or holder fields outside Registered",
            ));
        }
    };
    let change = NonZeroU64::new(decimal(&required(change, "lease.change")?, "lease.change")?)
        .ok_or_else(|| corrupt("lease.change", "must be positive"))?;
    let incarnation = RegistryIncarnation::new(decimal(
        &required(incarnation, "lease.incarnation")?,
        "lease.incarnation",
    )?)
    .map_err(|error| corrupt("lease.incarnation", error))?;
    let snapshot = RegistrySnapshot {
        database,
        revision: RegistryRevision { epoch, change },
        changed_at: SyncTime::from_unix_millis(
            changed_at.ok_or_else(|| corrupt("lease.changed_at", "missing timestamp"))?,
        ),
        lifecycle,
        grant,
    };
    let expected = LeaseContentDigest::new(required(digest, "lease.digest")?)
        .map_err(|error| corrupt("lease.digest", error))?;
    let projection = LeaseProjection::from_snapshot(snapshot, holder, incarnation)
        .map_err(|error| corrupt("lease.projection", error))?;
    if projection.digest() != &expected {
        return Err(corrupt("lease.digest", "snapshot digest differs"));
    }
    Ok(Some(projection))
}

fn parse_epoch(value: &str) -> Result<Epoch, StoreError> {
    Epoch::new(decimal(value, "lease.epoch")?).map_err(|error| corrupt("lease.epoch", error))
}
