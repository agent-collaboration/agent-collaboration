use crate::session_picker::details::details_client_inventory::*;
use serde_json::json;
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";
#[test]
fn unknown_native_history_cannot_acquire_restore_capability_from_launch_availability() {
    for (state, eligible) in [
        (json!({"kind":"stopped"}), true),
        (json!({"kind":"unknown","state":"future-state"}), false),
    ] {
        let summary=serde_json::from_value(json!({"origin":"claudeCodeBackground","target":{"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000042"},"workingDirectory":"/native","jobHandle":"separate-handle","name":null,"startedAt":1790162100_i64,"state":state,"peer":null,"status":null})).unwrap();
        let record = SessionPickerRecord::from_provider_summary(&summary, "Native Claude")
            .with_native_inventory_view(
                agent_session_inventory::picker_runtime_status::NativeCatalogueView::Stored,
            );
        let query = DetailsClientQuery {
            source: PickerSourceContext::DefaultHosted,
            record: Box::new(record),
        };
        let capabilities = DetailsClientCapabilities::Native {
            source: SourceNativeBinding {
                service_id: SERVICE.to_owned().try_into().unwrap(),
                service_epoch: EPOCH.to_owned().try_into().unwrap(),
            },
        };
        assert_eq!(
            capabilities.matches_query(&query),
            eligible,
            "unknown lifecycle grants no native restore authority"
        );
    }
}
