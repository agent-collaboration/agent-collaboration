//! Use the public bounded operator protocol for the exact owned test Host.
use super::offline_cli_fixture::TestResult;
use codex_router_host::{
    MAX_OPERATOR_FRAME_BYTES, OperatorFrame, OperatorRequest, decode_operator_frame,
    encode_operator_request,
};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub(super) async fn connect(socket: &Path) -> TestResult<UnixStream> {
    Ok(tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match UnixStream::connect(socket).await {
                Ok(stream) => return Ok(stream),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    tokio::task::yield_now().await
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await??)
}

pub(super) async fn await_ready(socket: &Path) -> TestResult {
    let mut stream = connect(socket).await?;
    stream
        .write_all(&encode_operator_request(&OperatorRequest::AwaitHostStart)?)
        .await?;
    stream.shutdown().await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(20),
        stream
            .take(u64::try_from(MAX_OPERATOR_FRAME_BYTES)? + 1)
            .read_to_end(&mut bytes),
    )
    .await??;
    if bytes.len() > MAX_OPERATOR_FRAME_BYTES {
        return Err("owned operator output exceeded its bound".into());
    }
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if let OperatorFrame::Terminal(response) = decode_operator_frame(line)? {
            if response.snapshot().hosted_readiness() == codex_router_host::HostedReadiness::Ready {
                return Ok(());
            }
            return Err("owned Host did not reach Ready".into());
        }
    }
    Err("owned Host omitted startup terminal result".into())
}

pub(super) async fn stop(socket: &Path) -> TestResult {
    let mut stream = connect(socket).await?;
    stream
        .write_all(&encode_operator_request(&OperatorRequest::StopHost)?)
        .await?;
    stream.shutdown().await?;
    Ok(())
}
