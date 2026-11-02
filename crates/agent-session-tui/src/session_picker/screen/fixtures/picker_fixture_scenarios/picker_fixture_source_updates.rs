//! A fixture source read waits on an owned socket, never on a timer or UI hook.
use crate::session_picker::SessionsPickerRecordLoader;
use crate::session_picker::SourceInventoryResult;
use crate::session_picker::screen::fixtures::picker_fixture_scenarios::FixtureSourceChange;
use crate::session_picker::screen::fixtures::picker_fixture_scenarios::PickerFixtureScenario;
use agent_session_inventory::picker_runtime_status::PickerRecordsSnapshot;
use agent_session_inventory::picker_runtime_status::PickerRuntimeCoverage;
use agent_session_inventory::source_inventory_request::InventoryPageProgress;
use agent_session_inventory::source_inventory_request::SourceInventoryRejection;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

struct SourceUpdateFixture {
    listener: UnixListener,
    path: PathBuf,
    released: Mutex<bool>,
}

impl Drop for SourceUpdateFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl SourceUpdateFixture {
    async fn release(&self) -> io::Result<()> {
        let mut released = self.released.lock().await;
        if *released {
            return Ok(());
        }
        let listener = tokio::net::UnixListener::from_std(self.listener.try_clone()?)?;
        let (stream, _) = listener.accept().await?;
        let mut bytes = Vec::new();
        let mut stream = stream.take(32);
        stream.read_to_end(&mut bytes).await?;
        if bytes != b"update\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixture update must contain exactly update newline",
            ));
        }
        // This acknowledges preparation only. The painted dropdown is the
        // publication evidence; the executor must observe it independently.
        stream.into_inner().write_all(b"prepared\n").await?;
        *released = true;
        Ok(())
    }
}

pub(in crate::session_picker) fn record_loader(
    scenario: Option<PickerFixtureScenario>,
    receipt_directory: Option<&Path>,
) -> io::Result<Option<SessionsPickerRecordLoader>> {
    let Some(PickerFixtureScenario::SourceOptions(change)) = scenario else {
        return Ok(None);
    };
    let directory = receipt_directory.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source-options fixture requires TUI_SESSIONS_FIXTURE_RECEIPT_DIR",
        )
    })?;
    let path = directory.join("source-update.sock");
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    let fixture = Arc::new(SourceUpdateFixture {
        listener,
        path,
        released: Mutex::new(false),
    });
    Ok(Some(Arc::new(move |request| {
        let fixture = fixture.clone();
        Box::pin(async move {
            match fixture.release().await {
                Ok(()) => SourceInventoryResult::Ready {
                    request,
                    binding: None,
                    snapshot: snapshot(change),
                    progress: InventoryPageProgress::Complete,
                },
                Err(_) => SourceInventoryResult::Rejected {
                    request,
                    reason: SourceInventoryRejection::InvalidInventory,
                },
            }
        })
    })))
}

fn snapshot(change: FixtureSourceChange) -> PickerRecordsSnapshot {
    PickerRecordsSnapshot {
        records: change.updated_records(),
        native_live_coverage: Vec::new(),
        runtime_coverage: PickerRuntimeCoverage::Available,
    }
}
