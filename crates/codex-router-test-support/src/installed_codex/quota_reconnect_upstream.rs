use super::{
    Arc, AtomicBool, ConcurrentUpstreamConfig, ConcurrentUpstreamSharedState, Duration, Instant,
    Ordering, PressureHandles, QuotaReconnectSqlitePressureConfig, TcpListener, join_result,
    looks_like_websocket_upgrade, respond_to_http_request, run_concurrent_mock_websocket_session,
    thread,
};

pub(super) fn run_concurrent_mock_upstream(
    listener: TcpListener,
    state: Arc<ConcurrentUpstreamSharedState>,
    shutdown: Arc<AtomicBool>,
    config: ConcurrentUpstreamConfig,
    sqlite_pressure: Option<QuotaReconnectSqlitePressureConfig>,
    pressure_handles: PressureHandles,
    completion: std::os::unix::net::UnixStream,
) -> Result<(), String> {
    let deadline = Instant::now()
        + Duration::from_secs(45)
            .saturating_add(config.hold_duration)
            .saturating_add(config.heartbeat_interval);
    let mut handles = Vec::new();
    loop {
        {
            let state_guard = state
                .state
                .lock()
                .map_err(|_| "concurrent upstream state mutex poisoned".to_owned())?;
            if state_guard.completed_sessions >= config.expected_upstream_sessions {
                break;
            }
        }
        if shutdown.load(Ordering::SeqCst) {
            return Err(
                "concurrent upstream shut down before expected sessions completed".to_owned(),
            );
        }
        if Instant::now() >= deadline {
            return Err("concurrent upstream timed out waiting for sessions".to_owned());
        }
        let mut descriptors = [
            rustix::event::PollFd::new(&listener, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&completion, rustix::event::PollFlags::IN),
        ];
        super::socket_readiness::wait_for_socket_readiness(&mut descriptors, Some(deadline))
            .map_err(|error| format!("concurrent upstream readiness failed: {error}"))?;
        if state
            .state
            .lock()
            .map_err(|_| "concurrent upstream state mutex poisoned".to_owned())?
            .completed_sessions
            >= config.expected_upstream_sessions
        {
            break;
        }
        if shutdown.load(Ordering::SeqCst) {
            return Err(
                "concurrent upstream shut down before expected sessions completed".to_owned(),
            );
        }
        match listener.accept() {
            Ok((stream, _peer)) => {
                stream.set_nonblocking(false).map_err(|error| {
                    format!("failed to restore concurrent upstream stream blocking mode: {error}")
                })?;
                if !looks_like_websocket_upgrade(&stream)? {
                    respond_to_http_request(stream)?;
                    let mut state_guard = state
                        .state
                        .lock()
                        .map_err(|_| "concurrent upstream state mutex poisoned".to_owned())?;
                    state_guard.http_probe_count = state_guard.http_probe_count.saturating_add(1);
                    continue;
                }
                let session_state = Arc::clone(&state);
                let session_shutdown = Arc::clone(&shutdown);
                let session_sqlite_pressure = sqlite_pressure.clone();
                let session_pressure_handles = Arc::clone(&pressure_handles);
                handles.push(
                    thread::Builder::new()
                        .name("codex-router-three-client-upstream-session".to_owned())
                        .spawn(move || {
                            run_concurrent_mock_websocket_session(
                                stream,
                                session_state,
                                session_shutdown,
                                config,
                                session_sqlite_pressure,
                                session_pressure_handles,
                            )
                        })
                        .map_err(|error| {
                            format!("failed to spawn concurrent upstream session: {error}")
                        })?,
                );
            }
            Err(error) => return Err(format!("concurrent upstream accept failed: {error}")),
        }
    }
    state.condition.notify_all();
    for (session_index, handle) in handles.into_iter().enumerate() {
        join_result(
            handle,
            &format!("concurrent upstream session {session_index}"),
        )?;
    }
    Ok(())
}
