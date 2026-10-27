//! Native RS256 issuance. The shared core remains the authority for claim checks.
use aws_lc_rs::{
    encoding::{AsDer, Pkcs8V1Der},
    rsa::KeySize,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{AccessCaller, AccessJwk, AccessJwks, AccessPolicy};
use ring::{
    rand::SystemRandom,
    rsa::{KeyPair, PublicKeyComponents},
    signature::RSA_PKCS1_SHA256,
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum IssuerError {
    #[error("central signing key generation failed")]
    Generation,
    #[error("central signing key is invalid")]
    InvalidKey,
    #[error("central assertion encoding or signing failed")]
    Signing,
    #[error("central assertion expiry must be in the future")]
    Expiry,
    #[error(transparent)]
    Policy(#[from] collaboration_central_core::AccessRefusal),
}

pub struct SelfHostedIssuer {
    key: KeyPair,
    kid: String,
    policy: AccessPolicy,
}

impl SelfHostedIssuer {
    /// RSA generation is blocking; callers run this on the blocking pool.
    pub fn generate_signing_key() -> Result<Vec<u8>, IssuerError> {
        let key = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048)
            .map_err(|_| IssuerError::Generation)?;
        let encoded = AsDer::<Pkcs8V1Der>::as_der(&key).map_err(|_| IssuerError::Generation)?;
        Ok(encoded.as_ref().to_vec())
    }

    pub fn from_pkcs8(bytes: &[u8], policy: AccessPolicy) -> Result<Self, IssuerError> {
        policy.validate()?;
        let key = KeyPair::from_pkcs8(bytes).map_err(|_| IssuerError::InvalidKey)?;
        let kid = URL_SAFE_NO_PAD.encode(ring::digest::digest(
            &ring::digest::SHA256,
            key.public().as_ref(),
        ));
        Ok(Self { key, kid, policy })
    }

    pub fn jwks(&self) -> AccessJwks {
        let public: PublicKeyComponents<Vec<u8>> = self.key.public().into();
        AccessJwks {
            keys: vec![AccessJwk {
                kid: self.kid.clone(),
                kty: "RSA".into(),
                n: URL_SAFE_NO_PAD.encode(public.n),
                e: URL_SAFE_NO_PAD.encode(public.e),
                alg: Some("RS256".into()),
                key_use: Some("sig".into()),
            }],
        }
    }

    pub fn mint(
        &self,
        caller: &AccessCaller,
        now: chrono::DateTime<chrono::Utc>,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<String, IssuerError> {
        if expires_at <= now {
            return Err(IssuerError::Expiry);
        }
        let (audience, common_name, email, subject) = match caller {
            AccessCaller::Machine { client_id } => (
                &self.policy.session_audience,
                Some(client_id.as_str()),
                None,
                "",
            ),
            AccessCaller::Owner { email } => {
                if email != &self.policy.owner_identity {
                    return Err(collaboration_central_core::AccessRefusal::WrongOwner.into());
                }
                (
                    &self.policy.admin_audience,
                    None,
                    Some(email.as_str()),
                    "self-hosted-owner",
                )
            }
        };
        let header = encode(&AssertionHeader {
            alg: "RS256",
            kid: &self.kid,
            typ: "JWT",
        })?;
        let claims = encode(&AssertionClaims {
            iss: &self.policy.issuer,
            aud: audience,
            exp: expires_at.timestamp(),
            nbf: now.timestamp(),
            assertion_type: "app",
            sub: subject,
            common_name,
            email,
        })?;
        let signed = format!("{header}.{claims}");
        let mut signature = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signed.as_bytes(),
                &mut signature,
            )
            .map_err(|_| IssuerError::Signing)?;
        Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }
}

fn encode(value: &impl Serialize) -> Result<String, IssuerError> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).map_err(|_| IssuerError::Signing)?))
}
#[derive(Serialize)]
struct AssertionHeader<'a> {
    alg: &'a str,
    kid: &'a str,
    typ: &'a str,
}
#[derive(Serialize)]
struct AssertionClaims<'a> {
    iss: &'a str,
    aud: &'a str,
    exp: i64,
    nbf: i64,
    #[serde(rename = "type")]
    assertion_type: &'a str,
    sub: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    common_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<&'a str>,
}
