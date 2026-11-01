use codex_router_secret_store::classified_store_open::UnusableCause;
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshCredentialKeeping {
    EncryptedStore,
    MemoryOnly,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UserCode(String);

impl TryFrom<String> for UserCode {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("invalid device user code");
        }
        Ok(Self(value))
    }
}
impl From<UserCode> for String {
    fn from(value: UserCode) -> Self {
        value.0
    }
}
impl UserCode {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for UserCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("UserCode([REDACTED])")
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct VerificationUri(String);
impl TryFrom<String> for VerificationUri {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let uri = reqwest::Url::parse(&value).map_err(|_| "invalid verification URI")?;
        if value.len() > 4096
            || !matches!(uri.scheme(), "http" | "https")
            || uri.host_str().is_none()
            || !uri.username().is_empty()
            || uri.password().is_some()
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            || uri.query_pairs().any(|(name, _)| {
                matches!(
                    name.as_ref(),
                    "code" | "user_code" | "device_code" | "access_token" | "refresh_token"
                )
            })
            || uri.fragment().is_some()
        {
            return Err("verification URI must contain no code or credentials");
        }
        Ok(Self(value))
    }
}
impl From<VerificationUri> for String {
    fn from(value: VerificationUri) -> Self {
        value.0
    }
}
impl VerificationUri {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for VerificationUri {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VerificationUri([REDACTED])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetLoginEvent {
    Code {
        user_code: UserCode,
        verification_uri: VerificationUri,
        keeping: RefreshCredentialKeeping,
    },
    LoggedIn {
        organization_id: OrganizationId,
        enrollment_id: EnrollmentId,
        keeping: RefreshCredentialKeeping,
    },
    Refused {
        reason: FleetLoginRefusal,
    },
    OutcomeUnknown {
        context: FleetLoginUnknown,
    },
}

impl FleetLoginEvent {
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::LoggedIn { .. } | Self::Refused { .. } | Self::OutcomeUnknown { .. }
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetLoginRefusal {
    AlreadyInOrganization { organization_id: OrganizationId },
    NotConfigured {},
    StoreUnusable { cause: UnusableCause },
    AccessDenied {},
    CodeExpired {},
    IssuerUnavailable {},
    InvalidIssuerResponse {},
    CredentialWriteFailed {},
    ConnectionUnavailable {},
    HostUnsupported {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetLoginUnknown {
    DeviceRedemption {
        organization_id: OrganizationId,
        installation: InstallationId,
    },
    CredentialPublication {
        organization_id: OrganizationId,
        enrollment_id: EnrollmentId,
        keeping: RefreshCredentialKeeping,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fleet_machine_login_frames_have_only_adopted_display_fields() {
        let event = FleetLoginEvent::Code {
            user_code: "ABCD-EFGH".to_owned().try_into().unwrap(),
            verification_uri: "https://sign-in.example.invalid/activate"
                .to_owned()
                .try_into()
                .unwrap(),
            keeping: RefreshCredentialKeeping::MemoryOnly,
        };
        let wire = json!({"state":"code","userCode":"ABCD-EFGH","verificationUri":"https://sign-in.example.invalid/activate","keeping":"memoryOnly"});
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FleetLoginEvent>(wire).unwrap(),
            event
        );
        assert!(!event.is_terminal());
        assert!(!format!("{event:?}").contains("ABCD-EFGH"));
        assert!(
            VerificationUri::try_from(
                "https://sign-in.example.invalid/activate?user_code=ABCD-EFGH".to_owned()
            )
            .is_err()
        );
    }

    #[test]
    fn fleet_machine_login_terminal_refusals_are_closed_kind_objects() {
        for reason in [
            FleetLoginRefusal::NotConfigured {},
            FleetLoginRefusal::AccessDenied {},
            FleetLoginRefusal::CodeExpired {},
            FleetLoginRefusal::IssuerUnavailable {},
            FleetLoginRefusal::InvalidIssuerResponse {},
            FleetLoginRefusal::CredentialWriteFailed {},
            FleetLoginRefusal::ConnectionUnavailable {},
            FleetLoginRefusal::HostUnsupported {},
            FleetLoginRefusal::AlreadyInOrganization {
                organization_id: "019f0000-0000-7000-8000-000000000001"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            },
            FleetLoginRefusal::StoreUnusable {
                cause: UnusableCause::InvalidDataKey,
            },
        ] {
            let event = FleetLoginEvent::Refused { reason };
            let mut wire = serde_json::to_value(&event).unwrap();
            assert!(wire["reason"]["kind"].is_string());
            assert_eq!(
                serde_json::from_value::<FleetLoginEvent>(wire.clone()).unwrap(),
                event
            );
            assert!(event.is_terminal());
            wire["reason"]["unexpected"] = json!(true);
            assert!(serde_json::from_value::<FleetLoginEvent>(wire).is_err());
        }
        assert!(
            serde_json::from_value::<FleetLoginEvent>(
                json!({"state":"refused","reason":{"kind":"futureRefusal"}})
            )
            .is_err()
        );
    }
}
