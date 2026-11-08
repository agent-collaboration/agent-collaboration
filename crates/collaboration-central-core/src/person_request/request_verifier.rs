//! Verify a person's request: the sign-in service's token, the DPoP proof bound
//! to it, and the signed intent of the operation the route received. The
//! result is a checked request, never a grant; the applying transaction
//! rechecks time, floors, permission and replay on its own clock.
use super::{
    admin_operations::{AdminOperation, AdminRoute},
    compact_jwt::{
        CompactJwt, MAX_SAFE_INTEGER, decode, from_json, sha256_url, uuid_v7, uuid_v7_millis,
    },
    refusal::PersonRequestRefusal,
};
use crate::{
    Ed25519Verification, IssuerKeySource, IssuerSignatureVerifier, SignatureValidity, VerifyFailure,
};
use durable_steps::Uuid;
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize};
use std::{future::Future, sync::Arc};

/// ES256 proof signatures; the host supplies the mechanics.
pub trait Es256Verification: Clone + Send + Sync + 'static {
    fn verify_es256(
        &self,
        key: &ProofJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send;
}

/// Public P-256 proof key only; a private `d` or any other member is refused.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProofJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
}

/// The deployment's trust: who signs person tokens, the audience they name for
/// central's admin surface, and central's configured external origin.
#[derive(Clone, Debug)]
pub struct PersonRequestPolicy {
    pub issuer: String,
    pub audience: String,
    /// `https://host[:port]` only; a route's `htu` is this plus its path.
    pub public_origin: String,
}

/// What arrived over HTTP, before anything is trusted.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonRequestInput {
    pub route: AdminRoute,
    pub method: String,
    pub authorization: String,
    pub proof: String,
    pub body: String,
}

/// The facts the applying transaction admits or refuses.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedPersonRequest {
    pub person_id: String,
    pub session_id: String,
    pub organization_id: OrganizationId,
    pub authorization_subject_id: String,
    pub person_revision: i64,
    pub organization_revision: i64,
    pub permission_policy_digest: String,
    pub permissions: Vec<String>,
    /// Seconds; absent when the session never passed a second factor.
    pub second_factor_at: Option<i64>,
    /// The fresh request ID (the proof's `jti`); only `organization_used_requests.id`.
    pub request_id: Uuid,
    pub request_time_ms: i64,
    pub proof_time_ms: i64,
    pub proof_key_thumbprint: String,
    pub intent_digest: String,
    /// min(exp, session deadline, snapshot deadline), milliseconds.
    pub effective_expiry_ms: i64,
    pub operation: AdminOperation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenHeader {
    alg: String,
    kid: String,
    typ: Option<String>,
}

/// RFC 7519 allows one audience string or an array of them.
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}
impl Audience {
    fn names(&self, expected: &str) -> bool {
        match self {
            Self::One(one) => one == expected,
            Self::Many(many) => many.iter().any(|one| one == expected),
        }
    }
}

#[derive(Deserialize)]
struct Confirmation {
    jkt: String,
}

#[derive(Deserialize)]
struct PersonClaims {
    iss: String,
    aud: Audience,
    sub: String,
    sid: String,
    iat: i64,
    exp: i64,
    cnf: Confirmation,
    token_profile: String,
    organization_id: String,
    authorization_subject_id: String,
    person_revision: i64,
    organization_revision: i64,
    permission_policy_digest: String,
    permissions: Vec<String>,
    second_factor_at: Option<i64>,
    #[serde(default)]
    second_factor_method: Option<String>,
    session_expires_at_ms: i64,
    authorization_snapshot_at_ms: i64,
    authorization_snapshot_valid_until_ms: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofHeader {
    typ: String,
    alg: String,
    jwk: ProofJwk,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofClaims {
    jti: String,
    htm: String,
    htu: String,
    iat: i64,
    ath: String,
    intent: String,
}

pub struct PersonRequestVerifier<Ed, Es, Keys> {
    signatures: IssuerSignatureVerifier<Ed, Keys>,
    proofs: Es,
    clock: Arc<dyn Clock>,
    policy: PersonRequestPolicy,
}

impl<Ed: Ed25519Verification, Es: Es256Verification, Keys: IssuerKeySource>
    PersonRequestVerifier<Ed, Es, Keys>
{
    #[must_use]
    pub fn new(
        signatures: IssuerSignatureVerifier<Ed, Keys>,
        proofs: Es,
        clock: Arc<dyn Clock>,
        policy: PersonRequestPolicy,
    ) -> Self {
        Self {
            signatures,
            proofs,
            clock,
            policy,
        }
    }

    pub async fn verify(
        &self,
        input: &PersonRequestInput,
    ) -> Result<VerifiedPersonRequest, PersonRequestRefusal> {
        let htu = expected_htu(&self.policy, input.route)?;
        let token = input
            .authorization
            .strip_prefix("DPoP ")
            .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace))
            .ok_or(PersonRequestRefusal::DpopRequired)?;
        let parsed_token = CompactJwt::parse(token)?;
        let header: TokenHeader = from_json(
            &parsed_token.header,
            PersonRequestRefusal::UnsupportedHeader,
        )?;
        if header.alg != "EdDSA"
            || header.kid.is_empty()
            || header
                .typ
                .as_ref()
                .is_some_and(|typ| typ != "JWT" && typ != "at+jwt")
        {
            return Err(PersonRequestRefusal::UnsupportedHeader);
        }
        match self
            .signatures
            .verify(&header.kid, &parsed_token.signed, &parsed_token.signature)
            .await
        {
            Ok(SignatureValidity::Valid) => {}
            Ok(SignatureValidity::Invalid) => return Err(PersonRequestRefusal::TokenSignature),
            Err(crate::IssuerSignatureRefusal::Keys(
                crate::IssuerKeyResolutionRefusal::KeySet(_),
            )) => {
                return Err(PersonRequestRefusal::IssuerKey);
            }
            Err(_) => return Err(PersonRequestRefusal::VerificationUnavailable),
        }
        let person: PersonClaims =
            from_json(&parsed_token.claims, PersonRequestRefusal::PersonClaims)?;
        let proof = CompactJwt::parse(&input.proof)?;
        let proof_header: ProofHeader =
            from_json(&proof.header, PersonRequestRefusal::UnsupportedHeader)?;
        if proof_header.typ != "dpop+jwt" || proof_header.alg != "ES256" {
            return Err(PersonRequestRefusal::UnsupportedHeader);
        }
        let thumbprint = proof_thumbprint(&proof_header.jwk)?;
        match self
            .proofs
            .verify_es256(&proof_header.jwk, &proof.signed, &proof.signature)
            .await
        {
            Ok(SignatureValidity::Valid) => {}
            Ok(SignatureValidity::Invalid) => return Err(PersonRequestRefusal::ProofSignature),
            Err(_) => return Err(PersonRequestRefusal::VerificationUnavailable),
        }
        let claims: ProofClaims = from_json(&proof.claims, PersonRequestRefusal::MalformedJwt)?;
        // Preliminary time checks on a clock sampled after the crypto; the
        // applying transaction samples again and rechecks every bound.
        let now_ms = self.clock.now().unix_millis();
        let effective_expiry_ms = validate_person(&person, &self.policy, now_ms)?;
        if thumbprint != person.cnf.jkt {
            return Err(PersonRequestRefusal::KeyBinding);
        }
        if input.method != "POST" || claims.htm != input.method {
            return Err(PersonRequestRefusal::Method);
        }
        if claims.htu != htu {
            return Err(PersonRequestRefusal::Target);
        }
        let proof_time_ms = claims
            .iat
            .checked_mul(1000)
            .ok_or(PersonRequestRefusal::ProofTime)?;
        if !in_request_window(proof_time_ms, now_ms) {
            return Err(PersonRequestRefusal::ProofTime);
        }
        let request_id = uuid_v7(&claims.jti, PersonRequestRefusal::RequestId)?;
        let request_time_ms = uuid_v7_millis(request_id).ok_or(PersonRequestRefusal::RequestId)?;
        if !in_request_window(request_time_ms, now_ms) {
            return Err(PersonRequestRefusal::RequestTime);
        }
        if claims.ath != sha256_url(token.as_bytes()) {
            return Err(PersonRequestRefusal::TokenHash);
        }
        let operation = AdminOperation::parse(input.route, input.body.as_bytes())?;
        let intent_digest = operation.intent_digest()?;
        if claims.intent != intent_digest {
            return Err(PersonRequestRefusal::Intent);
        }
        let organization_id: OrganizationId = person
            .organization_id
            .clone()
            .try_into()
            .map_err(|_| PersonRequestRefusal::PersonClaims)?;
        if operation.organization_id() != &organization_id {
            return Err(PersonRequestRefusal::OrganizationMismatch);
        }
        Ok(VerifiedPersonRequest {
            person_id: person.sub,
            session_id: person.sid,
            organization_id,
            authorization_subject_id: person.authorization_subject_id,
            person_revision: person.person_revision,
            organization_revision: person.organization_revision,
            permission_policy_digest: person.permission_policy_digest,
            permissions: person.permissions,
            second_factor_at: person.second_factor_at,
            request_id,
            request_time_ms,
            proof_time_ms,
            proof_key_thumbprint: thumbprint,
            intent_digest,
            effective_expiry_ms,
            operation,
        })
    }
}

/// `N − 65000 < T ≤ N + 5000` (T04 with the 5-second leeway on both sides).
pub(crate) fn in_request_window(timestamp_ms: i64, now_ms: i64) -> bool {
    timestamp_ms > now_ms - 65_000 && timestamp_ms <= now_ms + 5_000
}

fn expected_htu(
    policy: &PersonRequestPolicy,
    route: AdminRoute,
) -> Result<String, PersonRequestRefusal> {
    let origin =
        url::Url::parse(&policy.public_origin).map_err(|_| PersonRequestRefusal::Policy)?;
    if origin.scheme() != "https"
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
        || origin.path() != "/"
        || policy.issuer.is_empty()
        || policy.audience.is_empty()
    {
        return Err(PersonRequestRefusal::Policy);
    }
    Ok(format!(
        "{}{}",
        origin.origin().ascii_serialization(),
        route.path()
    ))
}

fn validate_person(
    person: &PersonClaims,
    policy: &PersonRequestPolicy,
    now_ms: i64,
) -> Result<i64, PersonRequestRefusal> {
    if person.iss != policy.issuer {
        return Err(PersonRequestRefusal::Issuer);
    }
    if !person.aud.names(&policy.audience) {
        return Err(PersonRequestRefusal::Audience);
    }
    if person.token_profile != "person-fleet" {
        return Err(PersonRequestRefusal::Profile);
    }
    for id in [
        &person.sub,
        &person.sid,
        &person.organization_id,
        &person.authorization_subject_id,
    ] {
        uuid_v7(id, PersonRequestRefusal::PersonClaims)?;
    }
    let revision = 1..=MAX_SAFE_INTEGER;
    if decode(&person.cnf.jkt, PersonRequestRefusal::PersonClaims)?.len() != 32
        || person.iat < 0
        || person.iat > now_ms.div_euclid(1000)
        || person.exp <= person.iat
        || !revision.contains(&person.person_revision)
        || !revision.contains(&person.organization_revision)
        || person.permission_policy_digest.is_empty()
        || person
            .second_factor_at
            .is_some_and(|at| !(0..=MAX_SAFE_INTEGER / 1000).contains(&at))
        || person.second_factor_at.is_some() != person.second_factor_method.is_some()
    {
        return Err(PersonRequestRefusal::PersonClaims);
    }
    let jwt_expiry_ms = person
        .exp
        .checked_mul(1000)
        .filter(|value| *value <= MAX_SAFE_INTEGER)
        .ok_or(PersonRequestRefusal::ExpiryBounds)?;
    let snapshot = person.authorization_snapshot_at_ms;
    let snapshot_deadline = person.authorization_snapshot_valid_until_ms;
    let session_deadline = person.session_expires_at_ms;
    if snapshot < 0
        || snapshot > now_ms
        || snapshot_deadline <= snapshot
        || snapshot_deadline > MAX_SAFE_INTEGER
        || snapshot_deadline - snapshot > 300_000
        || session_deadline <= snapshot
        || session_deadline > MAX_SAFE_INTEGER
    {
        return Err(PersonRequestRefusal::ExpiryBounds);
    }
    let effective_expiry_ms = jwt_expiry_ms.min(snapshot_deadline).min(session_deadline);
    if now_ms >= effective_expiry_ms {
        return Err(PersonRequestRefusal::Expired);
    }
    Ok(effective_expiry_ms)
}

fn proof_thumbprint(key: &ProofJwk) -> Result<String, PersonRequestRefusal> {
    if key.kty != "EC"
        || key.crv != "P-256"
        || decode(&key.x, PersonRequestRefusal::ProofKey)?.len() != 32
        || decode(&key.y, PersonRequestRefusal::ProofKey)?.len() != 32
    {
        return Err(PersonRequestRefusal::ProofKey);
    }
    // RFC 7638: exactly these members in lexicographic order, no whitespace.
    #[derive(Serialize)]
    struct Thumbprint<'a> {
        crv: &'a str,
        kty: &'a str,
        x: &'a str,
        y: &'a str,
    }
    let canonical = serde_json::to_vec(&Thumbprint {
        crv: &key.crv,
        kty: &key.kty,
        x: &key.x,
        y: &key.y,
    })
    .map_err(|_| PersonRequestRefusal::ProofKey)?;
    Ok(sha256_url(&canonical))
}
