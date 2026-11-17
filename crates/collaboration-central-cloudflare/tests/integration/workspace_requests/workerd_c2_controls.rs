//! External-only witness and eviction controls for the real compiled Worker.
use super::*;
use central_storage::SafeInteger;
use collaboration_central_core::identity_contract::ClientId;
use fleet_identity_protocol::{EnrollmentId, InstallationId, UnixSeconds};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum WitnessPhase {
    Connecting,
    Baseline,
    Evicting,
    Heartbeat,
    Deciding,
    Settling,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachmentSnapshot {
    pub organization_id: OrganizationId,
    pub machine_id: uuid::Uuid,
    pub installation_id: InstallationId,
    pub enrollment_id: EnrollmentId,
    pub client_id: ClientId,
    pub effective_expires_at: UnixSeconds,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WitnessRecord {
    pub sequence: u64,
    pub phase: WitnessPhase,
    pub instance_id: uuid::Uuid,
    pub event: WitnessEvent,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EnrollmentReadQuery {
    ByEnrollment,
    MachineByInstallation,
    ActiveEnrollment,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EnrollmentReadOutcome {
    Read,
    Unavailable,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RestoreOutcome {
    Admitted,
    Refused,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WitnessEvent {
    Constructor {
        object_id: String,
    },
    Upgrade {
        object_id: String,
        instance_id: uuid::Uuid,
        attachments: Vec<AttachmentSnapshot>,
    },
    Handler {
        object_id: String,
        instance_id: uuid::Uuid,
        attachment: AttachmentSnapshot,
    },
    EnrollmentRead {
        object_id: String,
        query: EnrollmentReadQuery,
        outcome: EnrollmentReadOutcome,
    },
    RestoreResult {
        object_id: String,
        correlation_id: central_link_protocol::CorrelationId,
        outcome: RestoreOutcome,
    },
    AlarmSnapshot {
        object_id: String,
        correlation_id: central_link_protocol::CorrelationId,
        #[serde(deserialize_with = "nullable_safe_integer")]
        earliest_due_ms: Option<SafeInteger>,
        #[serde(deserialize_with = "nullable_safe_integer")]
        alarm_ms: Option<SafeInteger>,
    },
}

fn nullable_safe_integer<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<SafeInteger>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(
    tag = "answer",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum BridgeReply {
    #[serde(rename = "c2Evicted")]
    C2Evicted {
        organization: OrganizationId,
    },
    #[serde(rename = "c2Witness")]
    C2Witness {
        events: Vec<WitnessRecord>,
    },
    ExternalTimeSet {
        set_time_ms: SafeInteger,
    },
    Refused {
        error: String,
    },
}

// Logs have other shapes; an answer envelope belongs to the command exchange.
pub(super) fn is_control_reply(line: &str) -> bool {
    #[derive(Deserialize)]
    struct Envelope {
        answer: String,
    }
    serde_json::from_str::<Envelope>(line).is_ok_and(|envelope| !envelope.answer.is_empty())
}

impl WorkerdHost {
    pub async fn open_c2(clock: Arc<dyn Clock>, plan: &Path) -> Result<Self, StoreFailure> {
        Self::open_configured(clock, Some(plan.to_owned()), true).await
    }
    async fn c2_control(&self, request: impl Serialize) -> Result<BridgeReply, StoreFailure> {
        let bytes = serde_json::to_vec(&request).map_err(|_| StoreFailure::Unavailable)?;
        let process = self.transport.process.clone();
        let line = tokio::task::spawn_blocking(move || process.exchange(&bytes))
            .await
            .map_err(|_| StoreFailure::Unavailable)??;
        let reply = serde_json::from_str(&line).map_err(|_| StoreFailure::Unavailable)?;
        if let BridgeReply::Refused { error } = &reply {
            // The transport reports refusal without publishing arbitrary bridge text.
            let _ = error;
            return Err(StoreFailure::Unavailable);
        }
        Ok(reply)
    }
    pub async fn c2_evict(&self, organization: &OrganizationId) -> Result<(), StoreFailure> {
        match self
            .c2_control(serde_json::json!({"c2Evict":organization}))
            .await?
        {
            BridgeReply::C2Evicted {
                organization: echoed,
            } if &echoed == organization => Ok(()),
            _ => Err(StoreFailure::Unavailable),
        }
    }
    pub async fn c2_witness(
        &self,
        phase: WitnessPhase,
    ) -> Result<Vec<WitnessRecord>, StoreFailure> {
        match self
            .c2_control(serde_json::json!({"c2Witness":{"phase":phase}}))
            .await?
        {
            BridgeReply::C2Witness { events } => Ok(events),
            _ => Err(StoreFailure::Unavailable),
        }
    }
    pub async fn set_external_time_ms(&self, milliseconds: i64) -> Result<(), StoreFailure> {
        let milliseconds = SafeInteger::new(milliseconds).map_err(|_| StoreFailure::Unavailable)?;
        match self
            .c2_control(serde_json::json!({"setExternalTimeMs":milliseconds}))
            .await?
        {
            BridgeReply::ExternalTimeSet { set_time_ms } if set_time_ms == milliseconds => Ok(()),
            _ => Err(StoreFailure::Unavailable),
        }
    }
}

pub(super) async fn open_fixture(clock: Arc<dyn Clock>) -> Result<WorkerdHost, StoreFailure> {
    let keys = signed::SignInKeys::start()
        .await
        .map_err(|_| StoreFailure::Unavailable)?;
    let root = tempfile::tempdir().map_err(|_| StoreFailure::Unavailable)?;
    let plan = root.path().join("signin-plan.json");
    let access = collaboration_central_conformance::fixture_policy();
    std::fs::write(&plan, serde_json::to_vec(&serde_json::json!({"textBindings":{
            "CENTRAL_SIGN_IN_ISSUER":signed::ISSUER,"CENTRAL_SIGN_IN_AUDIENCE":signed::AUDIENCE,
            "CENTRAL_MACHINE_AUDIENCE":signed::MACHINE_AUDIENCE,
            "CENTRAL_SIGN_IN_JWKS_URL":"https://signin.test/.well-known/jwks.json",
            "CENTRAL_ADMIN_AUDIENCE":signed::ADMIN_AUDIENCE,"CENTRAL_PUBLIC_ORIGIN":signed::PUBLIC_ORIGIN,
            "CENTRAL_SIGN_IN_SERVICE_ID":"019f0000-0000-7000-8000-00000000d290",
            "ACCESS_TEAM_DOMAIN":access.issuer.strip_prefix("https://").unwrap(),
            "ACCESS_SESSION_AUD":access.session_audience,"ACCESS_ADMIN_AUD":access.admin_audience,"ACCESS_OWNER_IDENTITY":access.owner_identity
        },"jwksFixtureUrl":keys.url})).map_err(|_| StoreFailure::Unavailable)?).map_err(|_| StoreFailure::Unavailable)?;
    let mut host = WorkerdHost::open_configured(clock, Some(plan), false).await?;
    host._fixture_keys = Some(keys);
    host._fixture_plan = Some(root);
    Ok(host)
}
