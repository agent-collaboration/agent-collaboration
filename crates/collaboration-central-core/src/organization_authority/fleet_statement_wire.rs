//! Decode the stamp profile without granting identity to unverified claims.
use super::fleet_statement_refusal::FleetOrganizationStatementRefusal;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fleet_identity_protocol::OrganizationId;
use serde::{Deserialize, Deserializer};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct OrganizationStatementDigest(String);

impl TryFrom<String> for OrganizationStatementDigest {
    type Error = FleetOrganizationStatementRefusal;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(FleetOrganizationStatementRefusal::Malformed);
        }
        Ok(Self(value))
    }
}

impl OrganizationStatementDigest {
    pub(super) fn of_compact_jws(statement: &str) -> Self {
        use sha2::{Digest, Sha256};
        Self(format!("{:x}", Sha256::digest(statement.as_bytes())))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StatementHeader {
    pub typ: String,
    pub alg: String,
    pub kid: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StatementClaims {
    pub purpose: String,
    pub version: u32,
    pub iss: String,
    pub aud: String,
    pub sub: OrganizationId,
    pub organization_id: OrganizationId,
    pub iat: StatementSeconds,
    pub exp: StatementSeconds,
    #[serde(default, deserialize_with = "present_predecessor")]
    pub replaces: Option<OrganizationStatementDigest>,
}

fn present_predecessor<'de, Decoder: Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<OrganizationStatementDigest>, Decoder::Error> {
    OrganizationStatementDigest::deserialize(decoder).map(Some)
}

#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "i64")]
pub(super) struct StatementSeconds(pub i64);

impl TryFrom<i64> for StatementSeconds {
    type Error = FleetOrganizationStatementRefusal;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if !(0..=9_007_199_254_740_991).contains(&value) {
            return Err(FleetOrganizationStatementRefusal::Malformed);
        }
        Ok(Self(value))
    }
}

pub(super) struct ParsedFleetStatement {
    pub header: StatementHeader,
    pub claims: StatementClaims,
    pub signed: Vec<u8>,
    pub signature: Vec<u8>,
}

impl ParsedFleetStatement {
    pub fn parse(statement: &str) -> Result<Self, FleetOrganizationStatementRefusal> {
        let mut parts = statement.split('.');
        let header = parts
            .next()
            .ok_or(FleetOrganizationStatementRefusal::Malformed)?;
        let claims = parts
            .next()
            .ok_or(FleetOrganizationStatementRefusal::Malformed)?;
        let signature = parts
            .next()
            .ok_or(FleetOrganizationStatementRefusal::Malformed)?;
        if parts.next().is_some() {
            return Err(FleetOrganizationStatementRefusal::Malformed);
        }
        Ok(Self {
            header: serde_json::from_slice(&decode(header)?)
                .map_err(|_| FleetOrganizationStatementRefusal::Malformed)?,
            claims: serde_json::from_slice(&decode(claims)?)
                .map_err(|_| FleetOrganizationStatementRefusal::Malformed)?,
            signed: format!("{header}.{claims}").into_bytes(),
            signature: decode(signature)?,
        })
    }
}

fn decode(value: &str) -> Result<Vec<u8>, FleetOrganizationStatementRefusal> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| FleetOrganizationStatementRefusal::Malformed)
}

/// A stored statement's expiry guard only; this does not establish signature trust.
pub(crate) fn stored_statement_unexpired(compact: &str, now: runtime_primitives::SyncTime) -> bool {
    ParsedFleetStatement::parse(compact)
        .is_ok_and(|parsed| now.unix_millis().div_euclid(1000) < parsed.claims.exp.0)
}
