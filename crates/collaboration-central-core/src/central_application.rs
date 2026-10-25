//! The application owns Access policy, verification order and observations.

use crate::{
    AccessCaller, AccessCheckOutcome, AccessJwks, AccessKeyCache, AccessKeySource, AccessPolicy,
    AccessRefusal, AccessRouteClass, CentralObservation, CentralPlatform, ParsedAccessAssertion,
    RegistryTime, VerifyFailure, authorize_access_route,
};
use std::sync::Arc;

pub struct CentralApplication<Platform: CentralPlatform> {
    pub(crate) platform: Platform,
    policy: AccessPolicy,
    access_keys: Arc<AccessKeyCache>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplicationCallFailure {
    #[error(transparent)]
    Access(#[from] AccessRefusal),
    #[error("object call could not be encoded")]
    Transport(durable_steps::CallFailure),
}

impl<Platform: CentralPlatform> CentralApplication<Platform> {
    pub fn new(
        platform: Platform,
        policy: AccessPolicy,
        access_keys: Arc<AccessKeyCache>,
    ) -> Result<Self, AccessRefusal> {
        policy.validate()?;
        Ok(Self {
            platform,
            policy,
            access_keys,
        })
    }

    pub async fn verify_access(
        &self,
        header: Option<&str>,
        path: &str,
    ) -> Result<AccessCaller, AccessRefusal> {
        let mut key_source = None;
        let result = self.verify_assertion(header, path, &mut key_source).await;
        let outcome = match &result {
            Ok(AccessCaller::Machine { .. }) => AccessCheckOutcome::AcceptedMachine,
            Ok(AccessCaller::Owner { .. }) => AccessCheckOutcome::AcceptedOwner,
            Err(refusal) => AccessCheckOutcome::Refused(refusal.clone()),
        };
        self.platform.observe(CentralObservation::AccessCheck {
            route_class: AccessRouteClass::for_path(path),
            outcome,
            key_source,
        });
        result
    }

    /// Bind an edge call to the caller that Access actually verified.
    pub async fn prepare_call<To>(
        &self,
        header: Option<&str>,
        path: &str,
        dispatch: crate::Dispatch<To>,
    ) -> Result<crate::SerializedObjectCall, ApplicationCallFailure>
    where
        To: crate::WireKind,
        crate::EdgeKind: crate::MayCall<To>,
    {
        let caller = self.verify_access(header, path).await?;
        let context = crate::CallerContext::for_verified_edge(caller)?;
        crate::serialize_object_call(&context, dispatch).map_err(ApplicationCallFailure::Transport)
    }

    pub(crate) async fn call_from_edge<To>(
        &self,
        caller: AccessCaller,
        to: To::Address,
        call: To::Call,
    ) -> crate::CallOutcome<To::Result>
    where
        To: crate::WireKind,
        crate::EdgeKind: crate::MayCall<To>,
    {
        let prepared = (|| {
            let context = crate::CallerContext::for_verified_edge(caller)
                .map_err(crate::object_call_envelope::encode_refusal)?;
            let call_id = crate::ObjectCallId::new(self.platform.ids().new_id().to_string())
                .map_err(crate::object_call_envelope::encode_refusal)?;
            crate::serialize_object_call(&context, crate::Dispatch::<To> { call_id, to, call })
        })();
        let result = match prepared {
            Ok(call) => self
                .platform
                .transport_object(call)
                .await
                .and_then(|reply| crate::deserialize_object_reply::<To>(&reply)),
            Err(failure) => Err(failure),
        };
        match result {
            Ok(reply) => crate::CallOutcome::Answered(reply),
            Err(failure) => crate::CallOutcome::Failed(failure),
        }
    }

    pub(crate) async fn call_from_owner<To, Call>(
        &self,
        owner: crate::OwnerIdentity,
        destination: To::Address,
        target: Call,
        request: Call::Request,
    ) -> crate::CallOutcome<Call::Answer>
    where
        To: crate::ObjectKind,
        Call: durable_steps::CallTarget,
        crate::EdgeKind: crate::MayCall<To>,
    {
        let prepared = (|| {
            let caller = crate::CallerContext::<crate::EdgeKind>::for_owner(owner);
            let identity = durable_steps::CallId {
                operation: durable_steps::OperationId::new(self.platform.ids().new_id())
                    .map_err(crate::object_call_envelope::encode_refusal)?,
                step: durable_steps::StepName::new("owner_operation")
                    .map_err(crate::object_call_envelope::encode_refusal)?,
            };
            crate::object_call_envelope::encode_shared_call::<crate::EdgeKind, To, Call>(
                &caller,
                destination,
                target,
                request,
                identity,
            )
        })();
        let reply = match prepared {
            Ok(call) => self
                .platform
                .transport_object(call)
                .await
                .and_then(|bytes| {
                    serde_json::from_slice(&bytes)
                        .map_err(|_| durable_steps::CallFailure::AnswerLost)
                }),
            Err(error) => Err(error),
        };
        match reply {
            Ok(answer) => crate::CallOutcome::Answered(answer),
            Err(failure) => crate::CallOutcome::Failed(failure),
        }
    }

    async fn verify_assertion(
        &self,
        header: Option<&str>,
        path: &str,
        key_source: &mut Option<AccessKeySource>,
    ) -> Result<AccessCaller, AccessRefusal> {
        let assertion = ParsedAccessAssertion::parse(header, &self.policy, path, self.now()?)?;
        let source = self.policy.certs_url();
        let (mut keys, mut fetched) = match self.access_keys.fresh(&source, self.now()?)? {
            Some(keys) => (keys, AccessKeySource::Cached),
            None => (
                self.fetch_keys(source.clone()).await?,
                AccessKeySource::Fetched,
            ),
        };
        if matches!(keys.key(assertion.kid()), Err(AccessRefusal::UnknownKey)) {
            keys = self.fetch_keys(source).await?;
            fetched = AccessKeySource::Refetched;
        }
        *key_source = Some(fetched);
        let validity = self
            .platform
            .verify_rs256(
                keys.key(assertion.kid())?,
                assertion.signed_bytes(),
                assertion.signature(),
            )
            .await
            .map_err(|failure| match failure {
                VerifyFailure::UnsupportedKey => AccessRefusal::InvalidKey,
                VerifyFailure::Crypto => AccessRefusal::BadSignature,
            })?;
        let caller = assertion.verified(validity)?;
        authorize_access_route(&self.policy, path, &caller)?;
        Ok(caller)
    }

    async fn fetch_keys(&self, source: String) -> Result<AccessJwks, AccessRefusal> {
        let response = self
            .platform
            .fetch_access_keys()
            .await
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
        self.access_keys.remember(source, response, self.now()?)
    }

    fn now(&self) -> Result<RegistryTime, AccessRefusal> {
        chrono::DateTime::from_timestamp_millis(self.platform.clock().now().unix_millis())
            .map(RegistryTime)
            .ok_or(AccessRefusal::InvalidCaller)
    }
}
