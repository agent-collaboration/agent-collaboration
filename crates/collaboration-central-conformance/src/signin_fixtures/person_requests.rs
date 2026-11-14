//! Person tokens and independent intent vectors, using real P-256 proofs.
use super::{ADMIN_AUDIENCE, ISSUER, PUBLIC_ORIGIN, sign};
use crate::ConformanceResult;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::KeyPair;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU32, Ordering};

pub const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000d201";
pub const OWNER: &str = "019f0000-0000-7000-8000-00000000d202";
pub const OWNER_SUBJECT: &str = "019f0000-0000-7000-8000-00000000d203";
pub const MEMBER: &str = "019f0000-0000-7000-8000-00000000d204";
pub const MEMBER_SUBJECT: &str = "019f0000-0000-7000-8000-00000000d205";
pub const SESSION: &str = "019f0000-0000-7000-8000-00000000d206";
pub const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";
pub const CREATE: &str = "/organization-admin/v1/create-workspace";
pub const LOOKUP: &str = "/organization-admin/v1/workspace-lookup";
pub const RECORDS: &str = "/organization-admin/v1/change-records";

/// Signed fixture identity; its key and credentials deliberately have no Debug.
pub struct Person {
    pub organization: String,
    pub id: String,
    pub subject: String,
    pub session: String,
    pub revision: i64,
    pub organization_revision: i64,
    pub policy_digest: String,
    pub permissions: Vec<String>,
    pub second_factor_at: Option<i64>,
    pub key: ProofKey,
}

static REQUESTS: AtomicU32 = AtomicU32::new(1);

/// A fresh UUIDv7 a second before the scenario's sampled time, with no clock read.
pub fn fresh_request_id(now_seconds: i64) -> ConformanceResult<String> {
    let millis = now_seconds
        .checked_mul(1000)
        .and_then(|at| at.checked_sub(1000))
        .filter(|at| (0..=0xffff_ffff_ffff).contains(at))
        .ok_or("fixture request time")?;
    let serial = REQUESTS
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |serial| {
            serial.checked_add(1)
        })
        .map_err(|_| "fixture request identities exhausted")?;
    Ok(format!(
        "{:08x}-{:04x}-7000-8000-{:012x}",
        millis >> 16,
        millis & 0xffff,
        serial
    ))
}

impl Person {
    pub fn owner(organization: &str, now_seconds: i64) -> ConformanceResult<Self> {
        Ok(Self {
            organization: organization.to_owned(),
            id: OWNER.to_owned(),
            subject: OWNER_SUBJECT.to_owned(),
            session: SESSION.to_owned(),
            revision: 4,
            organization_revision: 3,
            policy_digest: POLICY.to_owned(),
            permissions: vec![
                "organization:readAudit".to_owned(),
                "organization:view".to_owned(),
                "workspace:create".to_owned(),
            ],
            second_factor_at: Some(now_seconds - 60),
            key: ProofKey::generate()?,
        })
    }

    pub fn member(organization: &str, now_seconds: i64) -> ConformanceResult<Self> {
        let mut member = Self::owner(organization, now_seconds)?;
        member.id = MEMBER.to_owned();
        member.subject = MEMBER_SUBJECT.to_owned();
        member.revision = 1;
        member.permissions = vec!["organization:view".to_owned()];
        Ok(member)
    }

    pub fn token(
        &self,
        now_seconds: i64,
        adjust: &dyn Fn(&mut Value),
    ) -> ConformanceResult<String> {
        let mut claims = json!({
            "iss":ISSUER,"aud":ADMIN_AUDIENCE,"sub":self.id,"sid":self.session,
            "iat":now_seconds-10,"exp":now_seconds+290,"cnf":{"jkt":self.key.thumbprint()?},
            "token_profile":"person-fleet","organization_id":self.organization,
            "authorization_subject_id":self.subject,"person_revision":self.revision,
            "organization_revision":self.organization_revision,"permission_policy_digest":self.policy_digest,
            "permissions":self.permissions,
            "second_factor_at":self.second_factor_at,"second_factor_method":self.second_factor_at.map(|_| "totp"),
            "session_expires_at_ms":(now_seconds+86_400)*1000,
            "authorization_snapshot_at_ms":now_seconds*1000-10_000,
            "authorization_snapshot_valid_until_ms":now_seconds*1000+200_000
        });
        adjust(&mut claims);
        sign(
            &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
            &claims,
            7,
        )
    }

    pub fn headers(
        &self,
        path: &str,
        intent: &Value,
        jti: &str,
        now_seconds: i64,
    ) -> ConformanceResult<Vec<(&'static str, String)>> {
        self.headers_with(path, intent, jti, now_seconds, &|_| {})
    }

    pub fn headers_with(
        &self,
        path: &str,
        intent: &Value,
        jti: &str,
        now_seconds: i64,
        adjust: &dyn Fn(&mut Value),
    ) -> ConformanceResult<Vec<(&'static str, String)>> {
        let token = self.token(now_seconds, adjust)?;
        let proof = self.key.proof(&json!({
            "jti":jti,"htm":"POST","htu":format!("{PUBLIC_ORIGIN}{path}"),"iat":now_seconds-1,
            "ath":sha256_url(token.as_bytes()),"intent":sha256_url(canonical(intent).as_bytes())
        }))?;
        Ok(vec![
            ("authorization", format!("DPoP {token}")),
            ("dpop", proof),
        ])
    }
}

/// Explicit expected intent objects, independent of the production encoder.
pub fn create_intent(organization: &str, operation: &str, name: &str) -> Value {
    json!({"v":1,"operation":"CreateWorkspace","organization":organization,"target":organization,
        "operationId":operation,"params":{"name":name,"description":""}})
}
pub fn create_body(organization: &str, operation: &str, name: &str) -> Value {
    json!({"organizationId":organization,"operationId":operation,"name":name,"description":""})
}
pub fn lookup_intent(organization: &str, workspace: &str) -> Value {
    json!({"v":1,"operation":"WorkspaceLookup","organization":organization,"target":workspace,"operationId":null,"params":{}})
}
pub fn records_intent(organization: &str, after: Option<&str>, limit: u32) -> Value {
    json!({"v":1,"operation":"ReadChangeRecords","organization":organization,"target":organization,"operationId":null,"params":{"after":after,"limit":limit}})
}

/// A person's DPoP key, generated per test; its public JWK is what the token binds.
pub struct ProofKey {
    pair: ring::signature::EcdsaKeyPair,
    rng: ring::rand::SystemRandom,
}
impl ProofKey {
    pub fn generate() -> ConformanceResult<Self> {
        use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .map_err(|_| "proof key generation")?;
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .map_err(|_| "proof key import")?;
        Ok(Self { pair, rng })
    }
    pub fn jwk(&self) -> ConformanceResult<Value> {
        let point = self.pair.public_key().as_ref();
        let x = point.get(1..33).ok_or("proof key x")?;
        let y = point.get(33..65).ok_or("proof key y")?;
        Ok(
            json!({"kty":"EC","crv":"P-256","x":URL_SAFE_NO_PAD.encode(x),"y":URL_SAFE_NO_PAD.encode(y)}),
        )
    }
    /// RFC 7638 thumbprint, the token's `cnf.jkt`.
    pub fn thumbprint(&self) -> ConformanceResult<String> {
        use sha2::{Digest, Sha256};
        let jwk = self.jwk()?;
        let canonical = format!(
            r#"{{"crv":"P-256","kty":"EC","x":{},"y":{}}}"#,
            jwk.get("x").ok_or("x")?,
            jwk.get("y").ok_or("y")?
        );
        Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes())))
    }
    pub fn proof(&self, claims: &Value) -> ConformanceResult<String> {
        let header = json!({"typ":"dpop+jwt","alg":"ES256","jwk":self.jwk()?});
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims)?)
        );
        let signature = self
            .pair
            .sign(&self.rng, signed.as_bytes())
            .map_err(|_| "proof signature")?;
        Ok(format!(
            "{signed}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}

pub fn sha256_url(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}

/// RFC 8785 for the fixture's closed intent objects (ASCII keys, integers only).
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(members) => {
            let mut keys = members.keys().collect::<Vec<_>>();
            keys.sort();
            let inner = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        Value::String(key.clone()),
                        members.get(key).map(canonical).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{inner}}}")
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}
