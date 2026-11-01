//! Real native capability verdicts for the same fixed cases observed on workerd.
#![cfg(test)]

use collaboration_central_core::{CentralPlatform, SignatureValidity};
use collaboration_central_tokio::{DeliveryConfig, TokioCentralPlatform};
use runtime_primitives::{Clock, SyncTime};
use serde::Deserialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[path = "native_http/support.rs"]
mod configuration;

#[derive(Default)]
struct ControlledClock(AtomicUsize);

impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        self.0.fetch_add(1, Ordering::SeqCst);
        SyncTime::from_unix_millis(1_793_462_400_000)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignatureCase {
    name: String,
    public_key: String,
    message: String,
    signature: String,
    valid: bool,
}

fn decode_hex(value: &str) -> Result<Vec<u8>, configuration::TestError> {
    let mut bytes = Vec::new();
    for offset in (0..value.len()).step_by(2) {
        let digits = value.get(offset..offset + 2).ok_or("invalid fixture hex")?;
        bytes.push(u8::from_str_radix(digits, 16)?);
    }
    Ok(bytes)
}

// Oracle: RFC 8032 section 7.1, tests 1 and 2, plus named byte mutations.
// The actual TokioCentralPlatform method stays real; fixed clocks and no
// network request or timeout participate in this signature observation.
#[tokio::test(start_paused = true)]
async fn issuer_ed25519_capability_matches_fixed_public_vectors()
-> Result<(), configuration::TestError> {
    let directory = tempfile::tempdir()?;
    let clock = Arc::new(ControlledClock::default());
    let platform = TokioCentralPlatform::new(
        "https://unused.example.invalid/jwks",
        clock.clone(),
        |_| {},
        directory.path(),
        configuration::registry_config()?,
        DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .await?;
    let cases: Vec<SignatureCase> =
        serde_json::from_str(include_str!("fixtures/issuer_signature_vectors.json"))?;
    let before = clock.0.load(Ordering::SeqCst);
    let proof = async {
        for case in cases {
            let verdict = platform
                .verify_ed25519(
                    &decode_hex(&case.public_key)?,
                    &decode_hex(&case.message)?,
                    &decode_hex(&case.signature)?,
                )
                .await
                .map_err(|error| format!("signature capability unavailable: {error:?}"))?;
            let expected = if case.valid {
                SignatureValidity::Valid
            } else {
                SignatureValidity::Invalid
            };
            if verdict != expected {
                return Err(
                    format!("{}: expected {expected:?}, observed {verdict:?}", case.name).into(),
                );
            }
            println!("ISSUER_SIGNATURE_CASE {} {verdict:?}", case.name);
        }
        if clock.0.load(Ordering::SeqCst) != before {
            return Err("signature capability consulted the clock".into());
        }
        Ok::<_, configuration::TestError>(())
    }
    .await;
    let stopped = platform.shutdown().await;
    stopped?;
    proof
}
