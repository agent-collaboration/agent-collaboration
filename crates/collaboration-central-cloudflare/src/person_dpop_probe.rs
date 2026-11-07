//! Local conformance only. Validation time is injected; performance time is elapsed.
use collaboration_central_core::person_dpop_prototype::{
    DpopVerification, PersonDpopInput, PersonDpopPolicy, verify_person_request,
};
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use worker::{Request, Response, wasm_bindgen::JsCast};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    policy: PersonDpopPolicy,
    trusted_jwks: String,
    now_ms: i64,
    requests: Vec<PersonDpopInput>,
    repeat: u32,
}

struct FixedClock(i64);
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0)
    }
}

#[derive(Serialize)]
struct Measurement {
    outcomes: Vec<DpopVerification>,
    verifications: u32,
    elapsed_ms: f64,
    /// The local runtime exposes no documented per-invocation CPU counter.
    cpu_ms: Option<f64>,
}

pub(crate) async fn fetch(mut request: Request) -> worker::Result<Response> {
    let probe: Probe = request.json().await?;
    if probe.repeat == 0
        || probe.repeat > 10_000
        || probe.requests.is_empty()
        || probe.requests.len() > 100
    {
        return Response::error("invalid bounded prototype workload", 400);
    }
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let performance = global
        .performance()
        .ok_or_else(|| worker::Error::RustError("performance clock unavailable".into()))?;
    let mut outcomes = Vec::new();
    let clock = FixedClock(probe.now_ms);
    let start_ms = performance.now();
    let mut verifications = 0;
    for repetition in 0..probe.repeat {
        for input in &probe.requests {
            let result = verify_person_request(
                &crate::person_dpop_crypto::WorkerPersonDpopCrypto,
                &clock,
                &probe.policy,
                &probe.trusted_jwks,
                input,
            )
            .await;
            if repetition == 0 {
                outcomes.push(result);
            }
            verifications += 1;
        }
    }
    Response::from_json(&Measurement {
        outcomes,
        verifications,
        elapsed_ms: performance.now() - start_ms,
        cpu_ms: None,
    })
}
