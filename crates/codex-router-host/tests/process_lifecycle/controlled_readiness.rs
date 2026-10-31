//! Real native and Remote Control phases keep independent virtual deadlines.
use super::*;

#[path = "readiness_peer.rs"]
mod readiness_peer;

#[tokio::test]
async fn app_server_readiness_gives_remote_control_its_own_deadline_after_native_ready()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new("composed-readiness")?;
    let socket_path = directory.path().join("app-server.sock");
    let peer = readiness_peer::start(&socket_path)?;

    let event_file = directory.path().join("events.log");
    let identity = executable_identity(&std::env::current_exe()?).await?;
    let mut command = signal_fixture_command(SignalFixtureMode::ExitOnTerminate, &event_file)?;
    let mut app_server = AppServerChild::spawn(&mut command, identity, "1.2.3".to_owned())?;
    wait_for_fixture_ready(&event_file).await?;
    let readiness = {
        let observing = app_server.await_readiness(
            &socket_path,
            Duration::from_millis(120),
            Duration::from_millis(120),
        );
        tokio::pin!(observing);
        readiness_peer::await_stage(observing.as_mut(), peer.native_requested).await?;
        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(80)).await;
        tokio::time::resume();
        peer.release_native
            .send(())
            .map_err(|_| "native response gate closed")?;
        readiness_peer::await_stage(observing.as_mut(), peer.remote_requested).await?;
        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(80)).await;
        tokio::time::resume();
        peer.release_remote
            .send(())
            .map_err(|_| "Remote Control gate closed")?;
        observing.await?
    };
    check_equal(
        readiness,
        AppServerReadiness::Ready {
            running_version: "1.2.3".to_owned(),
            remote_control_server_name: Some(
                "fixture"
                    .to_owned()
                    .try_into()
                    .expect("valid fixture Remote Control name"),
            ),
        },
        "native and Remote Control convergence must receive separate deadlines",
    )?;
    check_equal(
        app_server.shutdown().await?,
        ShutdownOutcome::Graceful,
        "readiness fixture child must stop gracefully",
    )?;
    peer.task.await?.map_err(std::io::Error::other)?;
    Ok(())
}
