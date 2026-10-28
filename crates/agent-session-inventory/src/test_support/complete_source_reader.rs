use crate::sessions::SessionPickerRecord;
/// A test consumer exhausts real page continuations to preserve terminal scan oracles.
/// Production progressive publication is separately exercised through the actual worker/UI.
#[expect(
    clippy::expect_used,
    reason = "test page consumer asserts that qualified results retain their binding"
)]
pub async fn read_complete_source(
    loader: crate::source_inventory_request::SourceInventoryLoader,
    initial: crate::source_inventory_request::SourceInventoryRequest,
) -> crate::source_inventory_request::SourceInventoryResult {
    use crate::{
        source_inventory_request::InventoryPageProgress,
        source_inventory_request::SourceInventoryResult,
    };
    let mut request = initial.clone();
    let mut rows: Vec<SessionPickerRecord> = Vec::new();
    loop {
        match loader(request.clone()).await {
            SourceInventoryResult::Qualified { continuation, .. } => {
                request.continuation = Some(continuation);
            }
            SourceInventoryResult::Rejected { reason, .. } => {
                return SourceInventoryResult::Rejected {
                    request: initial,
                    reason,
                };
            }
            SourceInventoryResult::Ready {
                binding,
                snapshot,
                progress,
                ..
            } => {
                for row in snapshot.records {
                    if let Some(stored) = rows.iter_mut().find(|old| old.identity == row.identity) {
                        stored
                            .corroborate_native_loaded(row)
                            .expect("test consumer must use production cross-view corroboration");
                    } else {
                        rows.push(row);
                    }
                }
                match progress {
                    InventoryPageProgress::Complete => {
                        return SourceInventoryResult::Ready {
                            request: initial,
                            binding,
                            snapshot: crate::picker_runtime_status::PickerRecordsSnapshot {
                                native_live_coverage: snapshot.native_live_coverage,
                                records: rows,
                                runtime_coverage: snapshot.runtime_coverage,
                            },
                            progress: InventoryPageProgress::Complete,
                        };
                    }
                    InventoryPageProgress::MoreAvailable { continuation } => {
                        request.continuation = Some(continuation)
                    }
                }
            }
        }
    }
}
