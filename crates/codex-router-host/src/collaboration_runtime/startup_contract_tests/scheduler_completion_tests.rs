//! The Host's scheduler wakes on every completion a running Run waits for: a lifecycle
//! journal commit (a native `turn/completed`), the provider supervisor's published result,
//! and the observation re-check while observation is unavailable. Each source, triggered on
//! its own, wakes the idle scheduler for one more pass.
use super::start_host_with_providers;

/// Waits until the scheduler has gone to sleep `count` times, observed at its own sleep boundary.
async fn scheduler_asleep(observation: &collaboration_runtime::WorkerObservation, count: u64) {
    (observation.asleep_at_least(count)).await;
}

/// A native observation of a thread going idle, as the observer would record it.
fn thread_went_idle(service: &str) -> collaboration_protocol::LifecycleObservation {
    let endpoint = serde_json::json!({"serviceId":service,"endpointId":"codex-local"});
    serde_json::from_value(serde_json::json!({
        "observedAt":"2027-01-15T08:01:00.000Z","source":"nativeNotification",
        "scope":{"endpoint":endpoint,
            "generation":{"serviceEpoch":service,"generation":1},
            "observerId":"00000000-0000-4000-8000-000000000003"},
        "subject":{"kind":"thread","address":{"endpoint":endpoint,"nativeThreadId":"composition-thread"}},
        "change":{"kind":"threadStatus","status":{"type":"idle"},"ordering":"established"},
    }))
    .expect("lifecycle observation")
}

#[tokio::test]
async fn the_hosts_scheduler_wakes_on_each_completion_source() {
    let directory = tempfile::Builder::new()
        .prefix("host-scheduler-sources-")
        .tempdir_in("/tmp")
        .expect("private Host root");
    // An unavailable provider gives the Host its supervisor without launching anything.
    let unavailable = crate::ExternalProviderStartup::unavailable(
        collaboration_protocol::ProviderKind::Cursor,
        "disabled fixture provider".to_owned(),
        "enable fixture provider".to_owned(),
    )
    .expect("configured endpoint");
    let fixture = start_host_with_providers(false, directory, vec![unavailable]).await;
    let runtime = &fixture.runtime;
    let observation = runtime
        .schedule_observation
        .clone()
        .expect("the Host runs a scheduler");
    let journal = runtime.journal.clone().expect("the fixture journal opens");
    let supervisor = runtime
        .external_provider_supervisor
        .clone()
        .expect("the Host supervises its provider entry");
    let service = String::from(runtime.service_id().clone());
    scheduler_asleep(&observation, 1).await;

    let idle = observation.snapshot().sleeps;
    journal
        .append(
            &thread_went_idle(&service),
            runtime.clock.utc_now().timestamp(),
        )
        .await
        .expect("journal append");
    scheduler_asleep(&observation, idle + 1).await;
    let after_journal = observation.snapshot().sleeps;
    supervisor.announce_completion();
    scheduler_asleep(&observation, after_journal + 1).await;
    let after_supervisor = observation.snapshot().sleeps;
    runtime.observation_rechecks.recheck();
    scheduler_asleep(&observation, after_supervisor + 1).await;
    let after_recheck = observation.snapshot().sleeps;
    // The Host owns its stores again before it shuts down.
    drop((journal, supervisor));
    fixture.runtime.shutdown().await.expect("Host shutdown");

    assert!(after_journal > idle, "a journal commit wakes the scheduler");
    assert!(
        after_supervisor > after_journal,
        "a provider completion wakes the scheduler"
    );
    assert!(
        after_recheck > after_supervisor,
        "an observation re-check wakes the scheduler"
    );
}
