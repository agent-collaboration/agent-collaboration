use super::*;

#[tokio::test]
async fn a_pending_fleet_read_stays_in_the_status_future_while_lifecycle_work_is_ready() {
    // Arrange: real status probes, with only the slow read completion controlled.
    let directory = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    drop(listener);
    let config = HostConfig::new(crate::HostConfigInputs {
        coordination_paths: crate::HostCoordinationPaths::new(
            directory.path().join("operator.sock"),
            directory.path().join("host.lock"),
        ),
        router_endpoint: endpoint,
        mcp_bind: "127.0.0.1:0".parse().unwrap(),
        app_server_socket: directory.path().join("missing-native.sock"),
        managed_executable: directory.path().join("missing-codex"),
        deadlines: crate::HostDeadlines::production(),
    });
    let entered = Arc::new(tokio::sync::Notify::new());
    let reader_entered = entered.clone();
    let (release, released) = tokio::sync::oneshot::channel();
    let observation = observe_status(StatusObservationInputs {
        config,
        router_ownership: crate::RouterOwnership::External,
        owned_router_child_present: false,
        running_identity: None,
        identity_deadline: std::time::Duration::from_secs(1),
        observe_installed_identity: false,
        router_executable_observer: Arc::new(tokio::sync::Mutex::new(
            crate::RouterExecutableObserver::capture(
                Err(std::io::Error::other("test executable unavailable")),
                "test".to_owned(),
            ),
        )),
        fleet_observation: Box::pin(async move {
            reader_entered.notify_one();
            released.await.unwrap();
            crate::FleetHostObservation::Unavailable {
                message: "controlled observation result".to_owned(),
            }
        }),
    });
    tokio::pin!(observation);

    // Act: the lifecycle completion arm must not run before the slow read finishes.
    tokio::select! {
        biased;
        _ = &mut observation => panic!("status completed before its Fleet observation; read remains in lifecycle arm"),
        _ = entered.notified() => {},
    }
    // Another ready lifecycle event remains selectable while the read is held.
    tokio::select! {
        biased;
        _ = &mut observation => panic!("held Fleet read was completed without release"),
        () = std::future::ready(()) => {},
    }
    release.send(()).unwrap();

    // Assert: only the completed background observation carries the Fleet result.
    let completed = observation.await;
    assert!(
        matches!(completed.fleet, crate::FleetHostObservation::Unavailable { message } if message == "controlled observation result")
    );
}
