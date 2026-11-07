//! Typed latest-link storage; the native session remains the state authority.
use super::{
    FleetCache, RouterLocalStorageError, Utc, invalid, parse_time, parse_u64, validate_id,
    validate_times,
};
use board_sync_protocol::{FleetLinkDrop, FleetLinkFailure, FleetLinkRecord, FleetLinkState};
use fleet_sync_protocol::PeerId;

#[path = "fleet_link_details.rs"]
mod details;

struct StoredLinkRow {
    id: String,
    peer_id: String,
    endpoint: String,
    instance: String,
    connection: String,
    revision: String,
    state_kind: String,
    state_since: String,
    connecting_attempt: Option<i64>,
    last_frame_at: Option<String>,
    refusal_kind: Option<String>,
    refusal_detail: Option<String>,
    last_drop_kind: Option<String>,
    last_drop_at: Option<String>,
    last_drop_connection: Option<String>,
    last_failure_kind: Option<String>,
    last_failure_detail: Option<String>,
    last_failure_at: Option<String>,
    created_at: String,
    updated_at: String,
}

async fn read_link(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<Option<(String, String, FleetLinkRecord)>, RouterLocalStorageError> {
    let rows = sqlx_turso::query_as!(StoredLinkRow, r#"SELECT id AS "id!: String", peer_id AS "peer_id!: String", endpoint AS "endpoint!: String", instance AS "instance!: String", connection AS "connection!: String", revision AS "revision!: String", state_kind AS "state_kind!: String", state_since AS "state_since!: String", connecting_attempt AS "connecting_attempt?: i64", last_frame_at AS "last_frame_at?: String", refusal_kind AS "refusal_kind?: String", refusal_detail AS "refusal_detail?: String", last_drop_kind AS "last_drop_kind?: String", last_drop_at AS "last_drop_at?: String", last_drop_connection AS "last_drop_connection?: String", last_failure_kind AS "last_failure_kind?: String", last_failure_detail AS "last_failure_detail?: String", last_failure_at AS "last_failure_at?: String", created_at AS "created_at!: String", updated_at AS "updated_at!: String" FROM fleet_link_state"#)
        .fetch_all(connection).await?;
    if rows.len() > 1 {
        return Err(invalid("fleetLink.singleton"));
    }
    rows.into_iter()
        .next()
        .map(StoredLinkRow::decode)
        .transpose()
}

impl StoredLinkRow {
    fn decode(self) -> Result<(String, String, FleetLinkRecord), RouterLocalStorageError> {
        validate_id(&self.id, "fleetLink.id")?;
        validate_id(&self.peer_id, "fleetLink.peerId")?;
        validate_times(&self.created_at, &self.updated_at)?;
        let since = parse_time(&self.state_since, "fleetLink.stateSince")?;
        let last_frame_at = self
            .last_frame_at
            .as_deref()
            .map(|value| parse_time(value, "fleetLink.lastFrameAt"))
            .transpose()?;
        let state = match (
            self.state_kind.as_str(),
            self.connecting_attempt,
            self.refusal_kind.as_deref(),
            self.refusal_detail.as_deref(),
        ) {
            ("disconnected", None, None, None) => FleetLinkState::Disconnected { since },
            ("connecting", Some(attempt), None, None) => FleetLinkState::Connecting {
                since,
                attempts: u64::try_from(attempt)
                    .ok()
                    .and_then(std::num::NonZeroU64::new)
                    .ok_or_else(|| invalid("fleetLink.connectingAttempt"))?,
            },
            ("connected", None, None, None) => FleetLinkState::Connected {
                since,
                last_contact: last_frame_at.ok_or_else(|| invalid("fleetLink.lastFrameAt"))?,
            },
            ("refused", None, Some(kind), detail) => FleetLinkState::Refused {
                at: since,
                reason: details::decode_refusal(kind, detail)?,
            },
            _ => return Err(invalid("fleetLink.state")),
        };
        let last_drop = match (
            self.last_drop_kind.as_deref(),
            self.last_drop_at.as_deref(),
            self.last_drop_connection.as_deref(),
        ) {
            (None, None, None) => None,
            (Some(kind), Some(at), Some(connection)) => Some(FleetLinkDrop {
                reason: details::decode_drop(kind)?,
                at: parse_time(at, "fleetLink.lastDropAt")?,
                connection: parse_u64(connection, "fleetLink.lastDropConnection")?,
            }),
            _ => return Err(invalid("fleetLink.lastDrop")),
        };
        let last_failure = match (
            self.last_failure_kind.as_deref(),
            self.last_failure_detail.as_deref(),
            self.last_failure_at.as_deref(),
        ) {
            (None, None, None) => None,
            (Some(kind), detail, Some(at)) => Some(FleetLinkFailure {
                reason: details::decode_failure(kind, detail)?,
                at: parse_time(at, "fleetLink.lastFailureAt")?,
            }),
            _ => return Err(invalid("fleetLink.lastFailure")),
        };
        let record = FleetLinkRecord {
            peer: PeerId::new(
                uuid::Uuid::parse_str(&self.peer_id).map_err(|_| invalid("fleetLink.peerId"))?,
            )
            .map_err(|_| invalid("fleetLink.peerId"))?,
            endpoint: self.endpoint,
            instance: parse_u64(&self.instance, "fleetLink.instance")?,
            connection: parse_u64(&self.connection, "fleetLink.connection")?,
            revision: parse_u64(&self.revision, "fleetLink.revision")?,
            last_frame_at,
            state,
            last_drop,
            last_failure,
        };
        if !record.is_valid() {
            return Err(invalid("fleetLink"));
        }
        Ok((self.id, self.created_at, record))
    }
}

impl FleetCache {
    pub async fn fleet_link(&self) -> Result<Option<FleetLinkRecord>, RouterLocalStorageError> {
        Ok(read_link(&mut *self.database.pool.acquire().await?)
            .await?
            .map(|(_, _, record)| record))
    }
    pub async fn save_fleet_link(
        &self,
        record: &FleetLinkRecord,
    ) -> Result<(), RouterLocalStorageError> {
        if !record.is_valid() {
            return Err(invalid("fleetLink"));
        }
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        let previous = read_link(&mut transaction).await?;
        let now = Utc::now().to_rfc3339();
        let (id, created_at) = previous.map_or_else(
            || (uuid::Uuid::now_v7().to_string(), now.clone()),
            |(id, created, _)| (id, created),
        );
        let peer = record.peer.to_string();
        let endpoint = &record.endpoint;
        let instance = record.instance.to_string();
        let connection = record.connection.to_string();
        let revision = record.revision.to_string();
        let (kind, since, attempt, refusal) = match &record.state {
            FleetLinkState::Disconnected { since } => ("disconnected", since, None, None),
            FleetLinkState::Connecting { since, attempts } => (
                "connecting",
                since,
                Some(
                    i64::try_from(attempts.get())
                        .map_err(|_| invalid("fleetLink.connectingAttempt"))?,
                ),
                None,
            ),
            FleetLinkState::Connected { since, .. } => ("connected", since, None, None),
            FleetLinkState::Refused { at, reason } => {
                ("refused", at, None, Some(details::encode_refusal(reason)))
            }
        };
        let since = since.to_rfc3339();
        let frame = record.last_frame_at.map(|at| at.to_rfc3339());
        let refusal_kind = refusal.as_ref().map(|(kind, _)| *kind);
        let refusal_detail = refusal.as_ref().and_then(|(_, detail)| detail.as_deref());
        let drop_kind = record
            .last_drop
            .as_ref()
            .map(|dropped| dropped.reason.as_str());
        let drop_at = record
            .last_drop
            .as_ref()
            .map(|dropped| dropped.at.to_rfc3339());
        let drop_connection = record
            .last_drop
            .as_ref()
            .map(|dropped| dropped.connection.to_string());
        let failure = record
            .last_failure
            .as_ref()
            .map(|failure| details::encode_failure(&failure.reason));
        let failure_kind = failure.as_ref().map(|(kind, _)| *kind);
        let failure_detail = failure.as_ref().and_then(|(_, detail)| detail.as_deref());
        let failure_at = record
            .last_failure
            .as_ref()
            .map(|failure| failure.at.to_rfc3339());
        sqlx_turso::query!("INSERT INTO fleet_link_state(id, peer_id, endpoint, instance, connection, revision, state_kind, state_since, connecting_attempt, last_frame_at, refusal_kind, refusal_detail, last_drop_kind, last_drop_at, last_drop_connection, last_failure_kind, last_failure_detail, last_failure_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET peer_id=excluded.peer_id, endpoint=excluded.endpoint, instance=excluded.instance, connection=excluded.connection, revision=excluded.revision, state_kind=excluded.state_kind, state_since=excluded.state_since, connecting_attempt=excluded.connecting_attempt, last_frame_at=excluded.last_frame_at, refusal_kind=excluded.refusal_kind, refusal_detail=excluded.refusal_detail, last_drop_kind=excluded.last_drop_kind, last_drop_at=excluded.last_drop_at, last_drop_connection=excluded.last_drop_connection, last_failure_kind=excluded.last_failure_kind, last_failure_detail=excluded.last_failure_detail, last_failure_at=excluded.last_failure_at, updated_at=excluded.updated_at", id, peer, endpoint, instance, connection, revision, kind, since, attempt, frame, refusal_kind, refusal_detail, drop_kind, drop_at, drop_connection, failure_kind, failure_detail, failure_at, created_at, now)
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(())
    }
}
