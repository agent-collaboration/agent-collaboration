//! C7 authorization selects an operation before any request body is read.
use crate::{
    ServiceTokenTimeRefusal, ServiceTokenTimeRule, SignatureValidity,
    identity_contract::ResourceUri, validate_service_token_times,
};
use fleet_identity_protocol::{OrganizationId, UnixSeconds};
use message_board::IdentityValidationError;
use std::collections::BTreeSet;

/// The OAuth provider owns these identifiers; pinned service clients need not
/// use the generated UUIDv7 format of an enrollment's machine client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceClientId(String);
impl TryFrom<String> for ServiceClientId {
    type Error = IdentityValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(IdentityValidationError {
                field: "client_id",
                requirement: "must identify a registered service client",
            });
        }
        Ok(Self(value))
    }
}
impl ServiceClientId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ServiceOperation {
    AdmitSignIn,
    PersonStanding,
    CentralLookup,
    DeviceLoginDecisions,
    MachineTokenDecisions,
    DeviceApprovalAuthorize,
    DeviceApprovalActivate,
    DeviceApprovalCancel,
    InitializeOrganization,
}
impl ServiceOperation {
    #[must_use]
    pub const fn scope(self) -> &'static str {
        match self {
            Self::AdmitSignIn => "admit-sign-in",
            Self::PersonStanding => "person-standing",
            Self::CentralLookup => "central-lookup",
            Self::DeviceLoginDecisions => "device-login-decisions",
            Self::MachineTokenDecisions => "machine-token-decisions",
            Self::DeviceApprovalAuthorize => "device-approval-authorize",
            Self::DeviceApprovalActivate => "device-approval-activate",
            Self::DeviceApprovalCancel => "device-approval-cancel",
            Self::InitializeOrganization => "initialize-organization",
        }
    }
    const fn time_rule(self) -> ServiceTokenTimeRule {
        match self {
            Self::AdmitSignIn
            | Self::PersonStanding
            | Self::CentralLookup
            | Self::DeviceLoginDecisions
            | Self::MachineTokenDecisions => ServiceTokenTimeRule::IssuerDecision,
            Self::DeviceApprovalAuthorize
            | Self::DeviceApprovalActivate
            | Self::DeviceApprovalCancel
            | Self::InitializeOrganization => ServiceTokenTimeRule::Service,
        }
    }
    const fn is_directory_call(self) -> bool {
        matches!(
            self,
            Self::DeviceApprovalAuthorize
                | Self::DeviceApprovalActivate
                | Self::DeviceApprovalCancel
                | Self::InitializeOrganization
        )
    }
    const fn lives_at_directory(self) -> bool {
        matches!(self, Self::CentralLookup | Self::DeviceLoginDecisions)
    }
}

/// This classification comes from trusted registration metadata, never a JWT
/// extension claiming its own kind. The production K7 adapter supplies it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisteredServiceClientKind {
    FleetDirectory,
    FleetCentral,
    Agent,
    Person,
    Machine,
    DeviceLogin,
}

pub enum ServiceCallerIdentity {
    Issuer,
    RegisteredClient {
        client_id: ServiceClientId,
        kind: RegisteredServiceClientKind,
    },
}

/// A domain projection, deliberately without Deserialize. It is not a second
/// K7 wire schema. The signature checker and registered-client resolver supply
/// these facts; neither body bytes nor token-supplied key URLs enter policy.
pub struct ServiceTokenFacts {
    pub issuer: ResourceUri,
    pub audience: ResourceUri,
    pub scope: String,
    pub issued_at: UnixSeconds,
    pub expires_at: UnixSeconds,
    pub caller: ServiceCallerIdentity,
}

#[derive(Clone)]
pub enum ServiceReceiverHome {
    Central { organization_id: OrganizationId },
    FleetDirectory,
}

pub enum PinnedServiceCaller {
    Issuer,
    FleetDirectory { client_id: ServiceClientId },
}

pub struct ServiceOperationTrust {
    pub operation: ServiceOperation,
    pub audience: ResourceUri,
    pub caller: PinnedServiceCaller,
}

pub struct ServiceReceiverPolicy {
    issuer: ResourceUri,
    home: ServiceReceiverHome,
    operations: Vec<ServiceOperationTrust>,
}

impl ServiceReceiverPolicy {
    pub fn new(
        issuer: ResourceUri,
        home: ServiceReceiverHome,
        operations: Vec<ServiceOperationTrust>,
    ) -> Result<Self, ServiceReceiverRefusal> {
        let mut names = BTreeSet::new();
        let mut audiences = BTreeSet::new();
        for trust in &operations {
            let home_is_directory = matches!(home, ServiceReceiverHome::FleetDirectory);
            let pin_is_directory =
                matches!(trust.caller, PinnedServiceCaller::FleetDirectory { .. });
            if trust.operation.lives_at_directory() != home_is_directory
                || trust.operation.is_directory_call() != pin_is_directory
                || !names.insert(trust.operation)
                || !audiences.insert(trust.audience.as_str())
            {
                return Err(ServiceReceiverRefusal::InvalidConfiguration);
            }
        }
        Ok(Self {
            issuer,
            home,
            operations,
        })
    }

    /// Only a valid signature over the facts' original token permits policy.
    /// The returned capability fixes which body parser may run next.
    pub fn authorize(
        &self,
        signature: SignatureValidity,
        facts: ServiceTokenFacts,
        now: UnixSeconds,
    ) -> Result<AuthorizedServiceOperation, ServiceReceiverRefusal> {
        if signature != SignatureValidity::Valid {
            return Err(ServiceReceiverRefusal::BadSignature);
        }
        if facts.issuer != self.issuer {
            return Err(ServiceReceiverRefusal::WrongIssuer);
        }
        let trust = self
            .operations
            .iter()
            .find(|trust| {
                trust.audience == facts.audience && trust.operation.scope() == facts.scope
            })
            .ok_or(ServiceReceiverRefusal::WrongOperation)?;
        validate_service_token_times(
            trust.operation.time_rule(),
            facts.issued_at,
            facts.expires_at,
            now,
        )?;
        match (&trust.caller, facts.caller) {
            (PinnedServiceCaller::Issuer, ServiceCallerIdentity::Issuer) => {}
            (
                PinnedServiceCaller::FleetDirectory {
                    client_id: expected,
                },
                ServiceCallerIdentity::RegisteredClient {
                    client_id,
                    kind: RegisteredServiceClientKind::FleetDirectory,
                },
            ) if *expected == client_id => {}
            _ => return Err(ServiceReceiverRefusal::WrongCaller),
        }
        Ok(AuthorizedServiceOperation {
            operation: trust.operation,
            home: self.home.clone(),
        })
    }
}

/// Not deserializable: an HTTP body cannot manufacture this capability.
pub struct AuthorizedServiceOperation {
    operation: ServiceOperation,
    home: ServiceReceiverHome,
}
impl AuthorizedServiceOperation {
    #[must_use]
    pub const fn operation(&self) -> ServiceOperation {
        self.operation
    }

    /// Run after the selected operation's typed decoder and before its port.
    pub fn require_served_organization(
        &self,
        organization: &OrganizationId,
    ) -> Result<(), ServiceReceiverRefusal> {
        match &self.home {
            ServiceReceiverHome::Central { organization_id } if organization_id != organization => {
                Err(ServiceReceiverRefusal::NotThisCentral)
            }
            ServiceReceiverHome::Central { .. } | ServiceReceiverHome::FleetDirectory => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ServiceReceiverRefusal {
    #[error("service receiver trust configuration is invalid")]
    InvalidConfiguration,
    #[error("service signature is invalid")]
    BadSignature,
    #[error("service issuer differs from pinned configuration")]
    WrongIssuer,
    #[error("credential and body do not name the selected operation")]
    WrongOperation,
    #[error("service caller differs from the pinned registration")]
    WrongCaller,
    #[error("this central does not serve the body's organization")]
    NotThisCentral,
    #[error(transparent)]
    Time(#[from] ServiceTokenTimeRefusal),
}
