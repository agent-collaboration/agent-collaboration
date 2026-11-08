//! Worker checks of the sign-in service's statements: WebCrypto Ed25519 over
//! the keys it publishes, fetched with the Workers fetch API.
use collaboration_central_core::{
    Ed25519Verification, IssuerKeyCache, IssuerKeySource, KeyFetchFailure, RawJwks,
    SignatureValidity, VerifyFailure,
    organization_authority::{
        FleetOrganizationStatementRefusal, FleetOrganizationStatementTrust,
        FleetOrganizationStatementVerifier, SignInVerification, VerifiedFleetOrganizationStatement,
    },
};
use collaboration_central_core::{
    IssuerSignatureVerifier,
    person_request::{
        Es256Verification, PersonRequestInput, PersonRequestPolicy, PersonRequestRefusal,
        PersonRequestVerifier, ProofJwk, VerifiedPersonRequest,
    },
};
use fleet_identity_protocol::OrganizationId;
use js_sys::{Array, Object, futures::JsFuture};
use runtime_primitives::Clock;
use serde::Serialize;
use std::{future::Future, sync::Arc};
use worker::{
    Env, Fetch,
    send::{SendFuture, SendWrapper},
    wasm_bindgen::{JsCast, JsValue},
};

/// The Fleet object's check, configured from the deployment's sign-in variables.
pub(crate) struct CloudflareStatementCheck {
    pub(crate) env: Arc<SendWrapper<Env>>,
    pub(crate) organization: OrganizationId,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) cache: Arc<IssuerKeyCache>,
}

impl SignInVerification for CloudflareStatementCheck {
    fn verify_statement<'a>(
        &'a self,
        statement: &'a str,
    ) -> durable_steps::BoxFuture<
        'a,
        Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>,
    > {
        Box::pin(async move {
            let variable = |name: &str| {
                self.env
                    .var(name)
                    .map(|value| value.to_string())
                    .map_err(|_| FleetOrganizationStatementRefusal::InvalidConfiguration)
            };
            let trust = FleetOrganizationStatementTrust::new(
                variable("CENTRAL_SIGN_IN_ISSUER")?,
                variable("CENTRAL_SIGN_IN_AUDIENCE")?,
                self.organization.clone(),
            )?;
            FleetOrganizationStatementVerifier::new(
                WorkerEd25519,
                WorkerSignInKeys {
                    source: variable("CENTRAL_SIGN_IN_JWKS_URL")?,
                },
                self.cache.clone(),
                self.clock.clone(),
                trust,
            )
            .verify(statement)
            .await
        })
    }

    fn verify_person_request<'a>(
        &'a self,
        input: &'a PersonRequestInput,
    ) -> durable_steps::BoxFuture<'a, Result<VerifiedPersonRequest, PersonRequestRefusal>> {
        Box::pin(async move {
            let variable = |name: &str| {
                self.env
                    .var(name)
                    .map(|value| value.to_string())
                    .map_err(|_| PersonRequestRefusal::Policy)
            };
            let keys = WorkerSignInKeys {
                source: variable("CENTRAL_SIGN_IN_JWKS_URL")?,
            };
            PersonRequestVerifier::new(
                IssuerSignatureVerifier::new(
                    WorkerEd25519,
                    keys,
                    self.cache.clone(),
                    self.clock.clone(),
                ),
                WorkerEs256,
                self.clock.clone(),
                PersonRequestPolicy {
                    issuer: variable("CENTRAL_SIGN_IN_ISSUER")?,
                    audience: variable("CENTRAL_ADMIN_AUDIENCE")?,
                    public_origin: variable("CENTRAL_PUBLIC_ORIGIN")?,
                },
            )
            .verify(input)
            .await
        })
    }

    fn sign_in_service(&self) -> Option<message_board::ServiceId> {
        let configured = self.env.var("CENTRAL_SIGN_IN_SERVICE_ID").ok()?.to_string();
        message_board::ServiceId::try_from(configured).ok()
    }
}

#[derive(Clone)]
struct WorkerEs256;
impl Es256Verification for WorkerEs256 {
    fn verify_es256(
        &self,
        key: &ProofJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        let key = key.clone();
        let signed = signed.to_vec();
        let signature = signature.to_vec();
        SendFuture::new(async move { verify_es256(&key, &signed, &signature).await })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EcAlgorithm {
    name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    named_curve: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<&'static str>,
}

async fn verify_es256(
    key: &ProofJwk,
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let subtle = global.crypto().map_err(|_| VerifyFailure::Crypto)?.subtle();
    let to_object = |value: JsValue| {
        value
            .dyn_into::<Object>()
            .map_err(|_| VerifyFailure::UnsupportedKey)
    };
    let jwk =
        to_object(serde_wasm_bindgen::to_value(key).map_err(|_| VerifyFailure::UnsupportedKey)?)?;
    let import = to_object(
        serde_wasm_bindgen::to_value(&EcAlgorithm {
            name: "ECDSA",
            named_curve: Some("P-256"),
            hash: None,
        })
        .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )?;
    let verification = to_object(
        serde_wasm_bindgen::to_value(&EcAlgorithm {
            name: "ECDSA",
            named_curve: None,
            hash: Some("SHA-256"),
        })
        .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )?;
    let usages = Array::new();
    usages.push(&JsValue::from_str("verify"));
    let public_key = JsFuture::from(
        subtle
            .import_key_with_object("jwk", &jwk, &import, false, &usages)
            .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )
    .await
    .map_err(|_| VerifyFailure::UnsupportedKey)?
    .dyn_into::<web_sys::CryptoKey>()
    .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let result = JsFuture::from(
        subtle
            .verify_with_object_and_u8_array_and_u8_array(
                &verification,
                &public_key,
                signature,
                signed,
            )
            .map_err(|_| VerifyFailure::Crypto)?,
    )
    .await
    .map_err(|_| VerifyFailure::Crypto)?;
    match result.as_bool() {
        Some(true) => Ok(SignatureValidity::Valid),
        Some(false) => Ok(SignatureValidity::Invalid),
        None => Err(VerifyFailure::Crypto),
    }
}

#[derive(Clone)]
struct WorkerEd25519;
impl Ed25519Verification for WorkerEd25519 {
    fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send {
        let public_key = public_key.to_vec();
        let signed = signed.to_vec();
        let signature = signature.to_vec();
        SendFuture::new(async move { verify_ed25519(&public_key, &signed, &signature).await })
    }
}

#[derive(Serialize)]
struct Algorithm {
    name: &'static str,
}

async fn verify_ed25519(
    public_key: &[u8],
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    // The host guarantees a Worker global, as in access_crypto.rs.
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let subtle = global.crypto().map_err(|_| VerifyFailure::Crypto)?.subtle();
    let algorithm = serde_wasm_bindgen::to_value(&Algorithm { name: "Ed25519" })
        .map_err(|_| VerifyFailure::UnsupportedKey)?
        .dyn_into::<Object>()
        .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let usages = Array::new();
    usages.push(&JsValue::from_str("verify"));
    let raw = js_sys::Uint8Array::from(public_key);
    let key = JsFuture::from(
        subtle
            .import_key_with_object("raw", &raw, &algorithm, false, &usages)
            .map_err(|_| VerifyFailure::UnsupportedKey)?,
    )
    .await
    .map_err(|_| VerifyFailure::UnsupportedKey)?
    .dyn_into::<web_sys::CryptoKey>()
    .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let result = JsFuture::from(
        subtle
            .verify_with_object_and_u8_array_and_u8_array(&algorithm, &key, signature, signed)
            .map_err(|_| VerifyFailure::Crypto)?,
    )
    .await
    .map_err(|_| VerifyFailure::Crypto)?;
    match result.as_bool() {
        Some(true) => Ok(SignatureValidity::Valid),
        Some(false) => Ok(SignatureValidity::Invalid),
        None => Err(VerifyFailure::Crypto),
    }
}

struct WorkerSignInKeys {
    source: String,
}
impl IssuerKeySource for WorkerSignInKeys {
    fn configured_source(&self) -> &str {
        &self.source
    }
    fn fetch_issuer_keys(&self) -> impl Future<Output = Result<RawJwks, KeyFetchFailure>> + Send {
        SendFuture::new(async {
            let mut response = Fetch::Url(
                self.source
                    .parse()
                    .map_err(|_| KeyFetchFailure::Unreachable)?,
            )
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
            let status = response.status_code();
            if !(200..300).contains(&status) {
                return Err(KeyFetchFailure::BadResponse { status });
            }
            let cache_control = response
                .headers()
                .get("cache-control")
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            let body = response
                .bytes()
                .await
                .map_err(|_| KeyFetchFailure::Unreachable)?;
            Ok(RawJwks {
                body,
                cache_control,
            })
        })
    }
}
