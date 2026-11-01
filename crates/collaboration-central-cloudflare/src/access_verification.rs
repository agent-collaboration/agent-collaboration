//! Construct the shared Access application with isolate-owned capabilities and cache.

use crate::platform::CloudflareCentralPlatform;
use collaboration_central_core::{
    AccessCheckOutcome, AccessKeyCache, AccessPolicy, AccessRefusal, CentralApplication,
    CentralObservation,
};
use std::sync::{Arc, LazyLock};
use worker::Env;

static ACCESS_KEYS: LazyLock<Arc<AccessKeyCache>> =
    LazyLock::new(|| Arc::new(AccessKeyCache::default()));

fn policy(env: &Env) -> worker::Result<AccessPolicy> {
    AccessPolicy::cloudflare(
        env.var("ACCESS_TEAM_DOMAIN")?.to_string(),
        env.var("ACCESS_SESSION_AUD")?.to_string(),
        env.var("ACCESS_ADMIN_AUD")?.to_string(),
        env.var("ACCESS_OWNER_IDENTITY")?.to_string(),
    )
    .map_err(|_| worker::Error::RustError("invalid Access policy".into()))
}

#[derive(Clone, Copy)]
pub(crate) enum VerificationLayer {
    Edge,
}

impl VerificationLayer {
    const fn label(self) -> &'static str {
        match self {
            Self::Edge => "edge",
        }
    }
}

/// Labels are log metadata; they do not influence Access decisions.
#[derive(Clone)]
pub(crate) struct InvocationLabels {
    invocation: String,
    ray: String,
}

impl InvocationLabels {
    #[cfg(feature = "conformance")]
    pub(crate) fn for_signature_probe() -> Self {
        Self {
            invocation: "issuer-signature-probe".into(),
            ray: String::new(),
        }
    }

    pub(crate) fn for_request(request: &worker::Request) -> worker::Result<Self> {
        Ok(Self {
            invocation: uuid::Uuid::now_v7().to_string(),
            ray: request.headers().get("cf-ray")?.unwrap_or_default(),
        })
    }

    pub(crate) fn observe(&self, layer: VerificationLayer, observation: CentralObservation) {
        let CentralObservation::AccessCheck {
            route_class,
            outcome,
            key_source,
        } = observation;
        let outcome = match outcome {
            AccessCheckOutcome::AcceptedMachine => "accepted machine".to_owned(),
            AccessCheckOutcome::AcceptedOwner => "accepted owner".to_owned(),
            AccessCheckOutcome::Refused(refusal) => format!("refused {refusal:?}"),
        };
        worker::console_log!(
            "central access: invocation={} layer={} ray={} route={route_class:?} keys={key_source:?} outcome={outcome}",
            self.invocation,
            layer.label(),
            log_safe(&self.ray),
        );
    }
}

#[cfg(feature = "conformance")]
pub(crate) async fn verify(
    env: &Env,
    header: Option<&str>,
    path: &str,
    labels: &InvocationLabels,
    layer: VerificationLayer,
) -> Result<collaboration_central_core::AccessCaller, AccessRefusal> {
    application(env, labels.clone(), layer)?
        .verify_access(header, path)
        .await
}

pub(crate) fn application(
    env: &Env,
    labels: InvocationLabels,
    layer: VerificationLayer,
) -> Result<CentralApplication<CloudflareCentralPlatform>, AccessRefusal> {
    let policy = policy(env).map_err(|_| AccessRefusal::InvalidCaller)?;
    let platform = CloudflareCentralPlatform::new(policy.certs_url(), labels, layer, env.clone());
    CentralApplication::new(platform, policy, Arc::clone(&ACCESS_KEYS))
}

fn log_safe(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else if value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        value
    } else {
        "unprintable"
    }
}
