use super::{jwt_validation::*, request_contract::*};
use crate::SignatureValidity;
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofHeader {
    typ: String,
    alg: String,
    jwk: Es256Jwk,
}
#[derive(Deserialize)]
struct ProofClaims {
    jti: String,
    htm: String,
    htu: String,
    iat: i64,
    ath: Option<String>,
    intent: String,
}

/// Cryptographically checked request, never a grant to apply an operation.
pub async fn verify_person_request(
    crypto: &impl DpopCrypto,
    clock: &dyn Clock,
    policy: &PersonDpopPolicy,
    trusted_jwks: &str,
    input: &PersonDpopInput,
) -> DpopVerification {
    match verify(crypto, clock, policy, trusted_jwks, input).await {
        Ok(request) => DpopVerification::Verified { request },
        Err(reason) => DpopVerification::Refused { reason },
    }
}

async fn verify(
    crypto: &impl DpopCrypto,
    clock: &dyn Clock,
    policy: &PersonDpopPolicy,
    trusted_jwks: &str,
    input: &PersonDpopInput,
) -> Result<VerifiedPersonRequest, DpopRefusal> {
    let target = url::Url::parse(&policy.public_htu).map_err(|_| DpopRefusal::Policy)?;
    if target.scheme() != "https"
        || target.host_str().is_none()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.query().is_some()
        || target.fragment().is_some()
        || target.as_str() != policy.public_htu
        || policy.issuer.is_empty()
        || policy.audience.is_empty()
    {
        return Err(DpopRefusal::Policy);
    }
    let token = input
        .authorization
        .strip_prefix("DPoP ")
        .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace))
        .ok_or(DpopRefusal::DpopRequired)?;
    let parsed_token = CompactJwt::parse(token)?;
    let token_header: TokenHeader =
        from_json(&parsed_token.header, DpopRefusal::UnsupportedHeader)?;
    if token_header.alg != "EdDSA"
        || token_header
            .typ
            .as_ref()
            .is_some_and(|typ| typ != "JWT" && typ != "at+jwt")
    {
        return Err(DpopRefusal::UnsupportedHeader);
    }
    if trusted_jwks.len() > 65_536 {
        return Err(DpopRefusal::IssuerKey);
    }
    let keys: IssuerJwks = from_json(trusted_jwks.as_bytes(), DpopRefusal::IssuerKey)?;
    let issuer_key = keys.select(&token_header.kid)?;
    valid_signature(
        crypto
            .verify_issuer(issuer_key, &parsed_token.signed, &parsed_token.signature)
            .await,
        DpopRefusal::TokenSignature,
    )?;
    let person: PersonClaims = from_json(&parsed_token.claims, DpopRefusal::PersonClaims)?;
    let proof = CompactJwt::parse(&input.proof)?;
    let proof_header: ProofHeader = from_json(&proof.header, DpopRefusal::UnsupportedHeader)?;
    if proof_header.typ != "dpop+jwt" || proof_header.alg != "ES256" {
        return Err(DpopRefusal::UnsupportedHeader);
    }
    let thumbprint = proof_thumbprint(&proof_header.jwk)?;
    valid_signature(
        crypto
            .verify_proof(&proof_header.jwk, &proof.signed, &proof.signature)
            .await,
        DpopRefusal::ProofSignature,
    )?;
    let claims: ProofClaims = from_json(&proof.claims, DpopRefusal::MalformedJwt)?;
    // Sample after async crypto, never reuse an earlier ingress time.
    let now_ms = clock.now().unix_millis();
    if !(65_000..=MAX_SAFE_INTEGER - 5_000).contains(&now_ms)
        || input.reject_through_ms < 0
        || input.reject_through_ms > MAX_SAFE_INTEGER
    {
        return Err(DpopRefusal::Policy);
    }
    let effective_expiry_ms = person.validate(policy, now_ms)?;
    if thumbprint != person.key_thumbprint() {
        return Err(DpopRefusal::KeyBinding);
    }
    if claims.htm != input.method || input.method != "POST" {
        return Err(DpopRefusal::Method);
    }
    // This specimen requires the configured canonical URL verbatim. Query values,
    // if a future operation has them, must be carried by that operation's intent.
    if claims.htu != policy.public_htu {
        return Err(DpopRefusal::Target);
    }
    let proof_time_ms = claims.iat.checked_mul(1000).ok_or(DpopRefusal::ProofTime)?;
    if !in_window(proof_time_ms, now_ms) {
        return Err(DpopRefusal::ProofTime);
    }
    let request_id = uuid_v7(&input.request_id, DpopRefusal::RequestId)?;
    if claims.jti != input.request_id {
        return Err(DpopRefusal::RequestId);
    }
    let request_time_ms = request_id
        .get_timestamp()
        .map(|timestamp| {
            timestamp.to_unix().0 * 1000 + u64::from(timestamp.to_unix().1 / 1_000_000)
        })
        .and_then(|millis| i64::try_from(millis).ok())
        .ok_or(DpopRefusal::RequestId)?;
    if !in_window(request_time_ms, now_ms) || request_time_ms <= input.reject_through_ms {
        return Err(DpopRefusal::RequestTime);
    }
    if claims.ath.as_deref() != Some(sha256_url(token.as_bytes()).as_str()) {
        return Err(DpopRefusal::TokenHash);
    }
    let (operation_id, intent_digest) = operation_intent(input, &person)?;
    if claims.intent != intent_digest {
        return Err(DpopRefusal::Intent);
    }
    Ok(VerifiedPersonRequest {
        person_id: person.sub,
        session_id: person.sid,
        organization_id: person.organization_id,
        request_id: input.request_id.clone(),
        operation_id,
        proof_key_thumbprint: thumbprint,
        intent_digest,
        effective_expiry_ms,
    })
}

fn in_window(timestamp_ms: i64, now_ms: i64) -> bool {
    timestamp_ms > now_ms - 65_000 && timestamp_ms <= now_ms + 5_000
}

fn valid_signature(
    result: Result<SignatureValidity, crate::VerifyFailure>,
    refusal: DpopRefusal,
) -> Result<(), DpopRefusal> {
    match result {
        Ok(SignatureValidity::Valid) => Ok(()),
        Ok(SignatureValidity::Invalid) => Err(refusal),
        Err(_) => Err(DpopRefusal::CryptoUnavailable),
    }
}

fn proof_thumbprint(key: &Es256Jwk) -> Result<String, DpopRefusal> {
    if key.kty != "EC"
        || key.crv != "P-256"
        || decode(&key.x, DpopRefusal::ProofKey)?.len() != 32
        || decode(&key.y, DpopRefusal::ProofKey)?.len() != 32
    {
        return Err(DpopRefusal::ProofKey);
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
    .map_err(|_| DpopRefusal::ProofKey)?;
    Ok(sha256_url(&canonical))
}

fn operation_intent(
    input: &PersonDpopInput,
    person: &PersonClaims,
) -> Result<(String, String), DpopRefusal> {
    if input.body.len() > 8192 {
        return Err(DpopRefusal::OperationBody);
    }
    let operation: PrototypeOperation =
        from_json(input.body.as_bytes(), DpopRefusal::OperationBody)?;
    match operation {
        PrototypeOperation::RenameWorkspace {
            organization_id,
            operation_id,
            workspace_id,
            display_name,
        } => {
            if organization_id != person.organization_id
                || display_name.is_empty()
                || display_name.len() > 256
            {
                return Err(DpopRefusal::OperationBody);
            }
            uuid_v7(&operation_id, DpopRefusal::OperationBody)?;
            uuid_v7(&workspace_id, DpopRefusal::OperationBody)?;
            // JSON tuple is the experiment's versioned, order-independent body projection.
            // Fresh transport IDs/tokens/proofs are deliberately excluded.
            let canonical = serde_json::to_vec(&(
                "fleet-person-intent-prototype-v1",
                &organization_id,
                &person.sub,
                &operation_id,
                "renameWorkspace",
                &workspace_id,
                &display_name,
            ))
            .map_err(|_| DpopRefusal::OperationBody)?;
            Ok((operation_id, sha256_url(&canonical)))
        }
    }
}
