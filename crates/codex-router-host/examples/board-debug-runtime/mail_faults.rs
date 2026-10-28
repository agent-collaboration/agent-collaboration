//! Private debug-example input selects a fixed cut on the live owning connection.
use codex_router_host::CollaborationRuntime;
use std::{io, path::Path};

pub async fn install(runtime: &CollaborationRuntime, directory: &Path) -> io::Result<()> {
    let path = directory.join(CollaborationRuntime::RUNTIME_TEST_MAIL_FAULT_FILE);
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let fault: interaction_storage::MailStoreTestFault =
        serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    runtime.runtime_test_install_mail_fault(fault).await
}
