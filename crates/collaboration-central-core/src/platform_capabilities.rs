//! Edge capabilities the runtime supplies to central: access keys, sessions, object transport.

use std::future::Future;

use crate::{
    AccessJwk, AdmissionOrigin, CentralObservation, ObjectPlatform, SerializedObjectCall,
    SignatureValidity,
};

pub struct RawJwks {
    pub body: Vec<u8>,
    pub cache_control: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyFetchFailure {
    Unreachable,
    BadResponse { status: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyFailure {
    UnsupportedKey,
    Crypto,
}

#[derive(Debug)]
pub enum CallOutcome<Reply> {
    Answered(Reply),
    Failed(durable_steps::CallFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpgradeFailure {
    NotAnUpgrade,
    Refused,
}

pub trait CentralPlatform: Clone + Send + Sync + 'static {
    type Host: ObjectPlatform;
    fn clock(&self) -> &dyn runtime_primitives::Clock;
    fn ids(&self) -> &dyn runtime_primitives::IdSource;
    fn transport_object(
        &self,
        call: SerializedObjectCall,
    ) -> impl Future<Output = Result<Vec<u8>, durable_steps::CallFailure>> + Send;
    fn upgrade_session(
        &self,
        request: http::request::Parts,
        binding: fleet_session::ConnectionBinding,
        admission_origin: AdmissionOrigin,
    ) -> impl Future<Output = Result<http::Response<axum::body::Body>, UpgradeFailure>> + Send;
    fn fetch_access_keys(&self) -> impl Future<Output = Result<RawJwks, KeyFetchFailure>> + Send;
    fn verify_rs256(
        &self,
        key: &AccessJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send;
    fn observe(&self, observation: CentralObservation);
}
