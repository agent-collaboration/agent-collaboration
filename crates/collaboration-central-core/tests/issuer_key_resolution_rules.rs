//! Refresh policy uses an event-driven source double and a controlled clock.
#![cfg(test)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    IssuerKeyCache, IssuerKeyId, IssuerKeyRefusal, IssuerKeyResolutionRefusal, IssuerKeySource,
    KeyFetchFailure, RawJwks, RegistryTime, resolve_issuer_key,
};
use runtime_primitives::{Clock, SyncTime};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    },
};
use tokio::sync::oneshot;

const NOW_MILLIS: i64 = 1_793_462_400_000;
const SOURCE: &str = "https://configured.example.invalid/jwks";

struct ControlledClock(AtomicI64);
impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

enum FetchStep {
    Answer(Result<RawJwks, KeyFetchFailure>),
    Held {
        entered: oneshot::Sender<()>,
        response: oneshot::Receiver<RawJwks>,
        received_at_millis: i64,
    },
}

struct ConfiguredSource {
    steps: Mutex<VecDeque<FetchStep>>,
    calls: AtomicUsize,
    clock: Arc<ControlledClock>,
}

impl ConfiguredSource {
    fn new(steps: Vec<FetchStep>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            calls: AtomicUsize::new(0),
            clock: Arc::new(ControlledClock(AtomicI64::new(NOW_MILLIS))),
        }
    }
}

impl IssuerKeySource for ConfiguredSource {
    fn configured_source(&self) -> &str {
        SOURCE
    }

    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let step = {
            self.steps
                .lock()
                .map_err(|_| KeyFetchFailure::Unreachable)?
                .pop_front()
                .ok_or(KeyFetchFailure::Unreachable)?
        };
        match step {
            FetchStep::Answer(answer) => answer,
            FetchStep::Held {
                entered,
                response,
                received_at_millis,
            } => {
                let _ = entered.send(());
                let response = response.await.map_err(|_| KeyFetchFailure::Unreachable)?;
                self.clock.0.store(received_at_millis, Ordering::SeqCst);
                Ok(response)
            }
        }
    }
}

fn key_id(value: &str) -> IssuerKeyId {
    IssuerKeyId::try_from(value.to_owned()).unwrap()
}

fn keys(identity: &str, public_key: [u8; 32]) -> RawJwks {
    RawJwks {
        body: serde_json::to_vec(&serde_json::json!({
            "keys":[{
                "kid":identity, "kty":"OKP", "crv":"Ed25519",
                "x":URL_SAFE_NO_PAD.encode(public_key)
            }]
        }))
        .unwrap(),
        cache_control: None,
    }
}

fn at(offset_millis: i64) -> RegistryTime {
    RegistryTime(chrono::DateTime::from_timestamp_millis(NOW_MILLIS + offset_millis).unwrap())
}

#[tokio::test(start_paused = true)]
async fn a_known_fresh_key_needs_no_second_fetch() {
    let source = ConfiguredSource::new(vec![FetchStep::Answer(Ok(keys("known", [1; 32])))]);
    let cache = IssuerKeyCache::default();
    for _ in 0..2 {
        let selected = resolve_issuer_key(&source, &cache, source.clock.as_ref(), &key_id("known"))
            .await
            .unwrap();
        assert_eq!(selected.public_key(), &[1; 32]);
    }
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn an_unknown_cached_key_refetches_once_then_refuses() {
    let source = ConfiguredSource::new(vec![
        FetchStep::Answer(Ok(keys("known", [1; 32]))),
        FetchStep::Answer(Ok(keys("known", [2; 32]))),
    ]);
    let cache = IssuerKeyCache::default();
    resolve_issuer_key(&source, &cache, source.clock.as_ref(), &key_id("known"))
        .await
        .unwrap();
    let refused =
        resolve_issuer_key(&source, &cache, source.clock.as_ref(), &key_id("missing")).await;
    assert!(matches!(
        refused,
        Err(IssuerKeyResolutionRefusal::KeySet(
            IssuerKeyRefusal::UnknownKey
        ))
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_cold_unknown_key_has_one_initial_fetch_and_one_refetch() {
    let source = ConfiguredSource::new(vec![
        FetchStep::Answer(Ok(keys("known", [1; 32]))),
        FetchStep::Answer(Ok(keys("known", [2; 32]))),
    ]);
    let refused = resolve_issuer_key(
        &source,
        &IssuerKeyCache::default(),
        source.clock.as_ref(),
        &key_id("missing"),
    )
    .await;
    assert!(matches!(
        refused,
        Err(IssuerKeyResolutionRefusal::KeySet(
            IssuerKeyRefusal::UnknownKey
        ))
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_rotated_key_is_selected_from_the_one_forced_refresh() {
    let source = ConfiguredSource::new(vec![
        FetchStep::Answer(Ok(keys("old", [1; 32]))),
        FetchStep::Answer(Ok(keys("new", [8; 32]))),
    ]);
    let cache = IssuerKeyCache::default();
    resolve_issuer_key(&source, &cache, source.clock.as_ref(), &key_id("old"))
        .await
        .unwrap();
    let selected = resolve_issuer_key(&source, &cache, source.clock.as_ref(), &key_id("new"))
        .await
        .unwrap();
    assert_eq!(selected.public_key(), &[8; 32]);
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_failed_expiry_refresh_never_falls_back_to_old_keys() {
    let source = ConfiguredSource::new(vec![FetchStep::Answer(Err(KeyFetchFailure::Unreachable))]);
    let cache = IssuerKeyCache::default();
    cache
        .remember(SOURCE.to_owned(), keys("old", [1; 32]), at(0))
        .unwrap();
    source.clock.0.store(NOW_MILLIS + 300_000, Ordering::SeqCst);
    let refused = resolve_issuer_key(&source, &cache, source.clock.as_ref(), &key_id("old")).await;
    assert!(matches!(
        refused,
        Err(IssuerKeyResolutionRefusal::Fetch {
            reason: KeyFetchFailure::Unreachable
        })
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn cache_lifetime_is_measured_from_the_key_response_receipt() {
    let (entered_sender, entered_receiver) = oneshot::channel();
    let (response_sender, response_receiver) = oneshot::channel();
    let source = Arc::new(ConfiguredSource::new(vec![FetchStep::Held {
        entered: entered_sender,
        response: response_receiver,
        received_at_millis: NOW_MILLIS + 100_000,
    }]));
    let cache = Arc::new(IssuerKeyCache::default());
    let resolving = {
        let source = source.clone();
        let cache = cache.clone();
        tokio::spawn(async move {
            resolve_issuer_key(&*source, &cache, source.clock.as_ref(), &key_id("known")).await
        })
    };
    entered_receiver.await.unwrap();
    assert!(response_sender.send(keys("known", [4; 32])).is_ok());
    let selected = resolving.await.unwrap().unwrap();
    assert_eq!(selected.public_key(), &[4; 32]);
    assert!(cache.fresh(SOURCE, at(399_999)).unwrap().is_some());
    assert!(cache.fresh(SOURCE, at(400_000)).unwrap().is_none());
}
