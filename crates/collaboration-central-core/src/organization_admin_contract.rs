//! Closed answers from a verified organization admin operation.
use fleet_contracts::WorkspaceView;
use fleet_signin::organization_authority::OrganizationChangeRecord;
use fleet_signin::person_request::AdmissionRefusal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The closed set of engine kinds an admin sees; test-support machines are hidden.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineKindName {
    WakeTimer,
    AutomationTimer,
    LegacyNoticeWindows,
    LegacyWorkspaceDrains,
    EngineSource,
}

/// The closed failure classes of central_work_attempts.last_failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineFailureClass {
    CorruptRow,
    ConstraintViolated,
    StatementRejected,
    Refused,
    OtherStoreUnavailable,
}

/// One registered engine kind in EngineHealth; zero counts are listed too.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineKindHealth {
    pub kind: EngineKindName,
    pub failing: u32,
    pub retired: u32,
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    pub oldest_first_failed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_failure: Option<EngineFailureClass>,
}

/// One retired failure record in EngineListRetired, ascending by attempt ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineRetiredItem {
    #[schemars(with = "String", extend("format" = "uuid"))]
    pub attempt_id: uuid::Uuid,
    pub kind: EngineKindName,
    pub subject_key: String,
    pub failure_class: EngineFailureClass,
    pub count: u32,
    #[schemars(with = "String", extend("format" = "date-time"))]
    pub retired_at: chrono::DateTime<chrono::Utc>,
    pub last_failure: Option<String>,
}

/// Verification failures become a bare 401; answered operations have a body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrganizationAdminAnswer {
    Answered(OrganizationAdminResult),
    Retained(RetainedOrganizationAdminAnswer),
    Unauthenticated,
}

/// Validated retained JSON travels to HTTP without another serialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetainedOrganizationAdminAnswer {
    text: String,
}
impl RetainedOrganizationAdminAnswer {
    pub(crate) fn validated(text: String) -> Self {
        Self { text }
    }
    pub(crate) fn into_text(self) -> String {
        self.text
    }
}

/// The closed body of a 200 on an admin route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationAdminResult {
    Applied {
        workspace: WorkspaceView,
    },
    Found {
        workspace: Option<WorkspaceView>,
    },
    Records {
        records: Vec<OrganizationChangeRecord>,
    },
    EngineHealth {
        kinds: Vec<EngineKindHealth>,
        #[schemars(with = "Option<String>", extend("format" = "date-time"))]
        alarm: Option<chrono::DateTime<chrono::Utc>>,
        alarm_missing: bool,
    },
    EngineRetired {
        items: Vec<EngineRetiredItem>,
        #[schemars(with = "Option<String>", extend("format" = "uuid"))]
        next: Option<uuid::Uuid>,
    },
    EngineRetried {},
    EngineDiscarded {},
    Refused {
        reason: AdminRefusal,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AdminRefusal {
    NameInUse,
    NotPermitted,
    SecondFactorRequired,
    RefreshRequired,
    AuthorizationSyncRequired,
    Expired,
    RequestTime,
    RequestReused,
    /// The same operation ID arrived with a different request.
    Conflict,
    Rejected,
    /// No failure record has this attempt ID (or a concurrent retry took it).
    NotFound,
    /// The record is failing, not retired.
    NotRetired,
    /// A notice unit or engine_source record has no subject to finish.
    DiscardUnsupported,
    /// The subject moved; nothing changed.
    Stale,
    /// Discard on a retired subject whose row will not decode; nothing changed.
    SubjectUnreadable,
}

impl From<AdmissionRefusal> for AdminRefusal {
    fn from(refusal: AdmissionRefusal) -> Self {
        match refusal {
            AdmissionRefusal::NotPermitted => Self::NotPermitted,
            AdmissionRefusal::SecondFactorRequired => Self::SecondFactorRequired,
            AdmissionRefusal::RefreshRequired => Self::RefreshRequired,
            AdmissionRefusal::AuthorizationSyncRequired => Self::AuthorizationSyncRequired,
            AdmissionRefusal::Expired => Self::Expired,
            AdmissionRefusal::RequestTime => Self::RequestTime,
            AdmissionRefusal::RequestReused => Self::RequestReused,
        }
    }
}
