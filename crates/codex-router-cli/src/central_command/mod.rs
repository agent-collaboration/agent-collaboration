mod arguments;
mod root_lock;
mod serve_configuration;

pub(crate) use arguments::CentralCommand;
use collaboration_central_tokio::TokioWorkspaceServer;
use std::{io::Write, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum CentralCommandError {
    #[error("{0}")]
    Arguments(String),
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("central local operation failed")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Server(#[from] collaboration_central_tokio::TokioWorkspaceServerError),
    #[error(transparent)]
    ServeConfiguration(#[from] serve_configuration::ServeConfigurationError),
    #[error("central local task failed")]
    LocalTask,
    #[error("central serve requires async dispatch")]
    AsyncDispatchRequired,
}

pub(crate) async fn run_central_command(
    command: CentralCommand,
    stdout: &mut (impl Write + Send),
) -> Result<(), CentralCommandError> {
    match command {
        CentralCommand::Help(help) => stdout.write_all(help.as_bytes())?,
        CentralCommand::Serve(arguments) => {
            let (config, _root_lock) = tokio::task::spawn_blocking(move || {
                let config = serve_configuration::ServeConfiguration::read(&arguments.config)?;
                let root_lock = root_lock::lock_root(&config.root)?;
                Ok::<_, CentralCommandError>((config, root_lock))
            })
            .await
            .map_err(|_| CentralCommandError::LocalTask)??;
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let server = TokioWorkspaceServer::start(
                &config.root,
                config.listen,
                config.sign_in,
                Arc::new(router_clock::SystemRuntimeClock::new()),
                nix::unistd::geteuid().as_raw(),
            )
            .await?;
            let running = async {
                writeln!(
                    stdout,
                    "central listening: http://{} (private sign-in socket {})",
                    server.address(),
                    server.sign_in_socket().display(),
                )?;
                stdout.flush()?;
                tokio::select! {
                    result = tokio::signal::ctrl_c() => result?,
                    _ = terminate.recv() => {}
                }
                Ok::<_, CentralCommandError>(())
            }
            .await;
            let shutdown = server.shutdown().await;
            running?;
            shutdown?;
            writeln!(stdout, "central stopped")?;
        }
    }
    Ok(())
}
