use crate::sessions::{
    RepositoryIdentity, SessionPickerRecord, SessionRecordQuery,
    load_session_records_for_query_with_identity,
};
use crate::sessions::{configured_inventory_loader, picker_runtime_inventory};
use crate::source_inventory_request::SourceInventoryLoader;

pub fn session_picker_record_loader(
    codex_home: Option<std::path::PathBuf>,
    current_dir: std::path::PathBuf,
    repository_identity: RepositoryIdentity,
    service_directory: Option<std::path::PathBuf>,
) -> SourceInventoryLoader {
    let runtime_inventory = std::sync::Arc::new(tokio::sync::Mutex::new(
        picker_runtime_inventory::PickerRuntimeInventory::default(),
    ));
    let expected_source = if service_directory.is_some() {
        crate::source_inventory_request::PickerSourceContext::DefaultHosted
    } else {
        crate::source_inventory_request::PickerSourceContext::LocalCodex
    };
    std::sync::Arc::new(move |request| {
        let codex_home = codex_home.clone();
        let current_dir = current_dir.clone();
        let repository_identity = repository_identity.clone();
        let service_directory = service_directory.clone();
        let runtime_inventory = std::sync::Arc::clone(&runtime_inventory);
        let expected_source = expected_source.clone();
        Box::pin(async move {
            use crate::{
                source_inventory_request::PickerSourceContext,
                source_inventory_request::SourceInventoryRejection,
                source_inventory_request::SourceInventoryResult,
            };
            if matches!(
                request.source_context,
                PickerSourceContext::ConfiguredHosted(_)
            ) {
                return configured_inventory_loader::load(request).await;
            }
            if request.source_context != expected_source {
                return SourceInventoryResult::Rejected {
                    request,
                    reason: SourceInventoryRejection::InvalidInventory,
                };
            }
            let query = request.query.clone();
            let include_empty_sessions = query.include_empty_sessions;
            let record_query = SessionRecordQuery::from_picker_query(query);
            let records = match load_session_records_for_query_with_identity(
                record_query,
                match codex_home.as_deref() {
                    Some(home) => home,
                    None => {
                        return SourceInventoryResult::Rejected {
                            request,
                            reason: SourceInventoryRejection::SourceUnavailable,
                        };
                    }
                },
                &current_dir,
                Some(repository_identity),
            )
            .await
            {
                Ok(records) => records,
                Err(_) => {
                    return SourceInventoryResult::Rejected {
                        request,
                        reason: SourceInventoryRejection::SourceUnavailable,
                    };
                }
            };
            let stored = records
                .iter()
                .map(SessionPickerRecord::from_record)
                .collect();
            let mut inventory = runtime_inventory.lock().await;
            let snapshot = match inventory
                .refresh(service_directory.as_deref(), stored, include_empty_sessions)
                .await
            {
                Ok(snapshot) => snapshot,
                Err(reason) => return SourceInventoryResult::Rejected { request, reason },
            };
            SourceInventoryResult::Ready {
                progress: crate::source_inventory_request::InventoryPageProgress::Complete,
                binding: None,
                request,
                snapshot,
            }
        })
    })
}
