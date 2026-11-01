//! Mounted controls observe one committed grouping and an isolated filter draft.
use super::*;
use crate::quota_test_support::frame_output::{SharedFrameOutput, dispatch_render_loop};
use crate::quota_test_support::{QuotaInputFrameDriver, current_quota_canvas};
use agent_collaboration_tui::{
    input::input_dispatch_receipt::InputDispatchReceiptSink,
    surfaces::{surface_handle::SurfaceStackHandle, surface_stack::SurfaceId},
};

#[derive(Default)]
struct FailedPreferenceStore(std::sync::Mutex<Vec<crate::QuotaGroupFacet>>);
impl crate::QuotaUiPreferenceStore for FailedPreferenceStore {
    fn load_group_facet(&self) -> crate::QuotaPreferenceFuture<'_, crate::QuotaGroupFacet> {
        Box::pin(async { crate::QuotaGroupFacet::None })
    }
    fn save_group_facet(
        &self,
        facet: crate::QuotaGroupFacet,
    ) -> crate::QuotaPreferenceFuture<'_, Result<(), crate::QuotaPreferenceSaveError>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(facet);
            Err(crate::QuotaPreferenceSaveError)
        })
    }
}

fn projected_group_facet(stack: SurfaceStackHandle) -> crate::QuotaGroupFacet {
    let snapshot = stack.snapshot();
    let actions = snapshot
        .surface(SurfaceId::Screen)
        .unwrap()
        .actions
        .presentations();
    let selected = [
        ("group-none", crate::QuotaGroupFacet::None),
        ("group-provider", crate::QuotaGroupFacet::Provider),
        ("group-status", crate::QuotaGroupFacet::Status),
    ]
    .into_iter()
    .filter_map(|(name, facet)| {
        let id = crate::quota_control_context::action_id(name).unwrap();
        let presentation = actions.iter().find(|action| action.id == id).unwrap();
        (presentation.checked == Some(true)).then_some(facet)
    })
    .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1, "exactly one real menu choice is checked");
    selected[0]
}

#[tokio::test]
async fn grouping_bindings_share_one_draft_and_committed_choice_at_four_grids() {
    use crate::QuotaGroupFacet::{None as Ungrouped, Provider, Status};
    for (width, height) in [(120usize, 36usize), (76, 30), (48, 24), (48, 12)] {
        let store = std::sync::Arc::new(FailedPreferenceStore::default());
        let (preferences, writer) = crate::QuotaGroupPreferences::load(store.clone()).await;
        let writer = tokio::spawn(writer.run());
        {
            let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
            let mounted = observed.clone();
            let observer = Handler::from(move |stack| *mounted.lock().unwrap() = Some(stack));
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let events = futures_util::stream::unfold(receiver, |mut receiver| async {
                receiver.recv().await.map(|event| (event, receiver))
            });
            let directory = tempfile::tempdir().unwrap();
            let receipt_path = directory.path().join("grouping-input.jsonl");
            let sink = InputDispatchReceiptSink::create(&receipt_path).unwrap();
            let mut component = element!(QuotaStatusComponent(
                view_model:quota_two_account_view_model(),width,height,
                initial_group_facet:preferences.current_facet(),group_preferences:Some(preferences.clone()),
                surface_stack_observer:Some(observer),dispatch_receipt:Some(sink.clone())
            ));
            let bytes = SharedFrameOutput::default();
            let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
            let frames = dispatch_render_loop(
                &mut component,
                MockTerminalConfig::with_events(events),
                bytes,
                writer,
            );
            tokio::pin!(frames);
            let mut input = QuotaInputFrameDriver::new(sender, receipt_path);
            let initial = current_quota_canvas(frames.as_mut()).await.to_string();
            let stack = observed
                .lock()
                .unwrap()
                .expect("actual InputOwner stack mounted");
            assert_eq!(projected_group_facet(stack), Ungrouped);

            let mut after_cycle = String::new();
            for expected in [Provider, Status, Ungrouped] {
                after_cycle = input
                    .step(frames.as_mut(), provider_filter_key(KeyCode::F(6)), |_| {
                        true
                    })
                    .await
                    .to_string();
                assert_eq!(
                    projected_group_facet(stack),
                    expected,
                    "F6 at {width}x{height}"
                );
                assert_eq!(
                    preferences.current_facet(),
                    expected,
                    "F6 updates run preference without waiting for a save"
                );
                let presentation = stack
                    .snapshot()
                    .surface(SurfaceId::Screen)
                    .unwrap()
                    .actions
                    .presentations()
                    .into_iter()
                    .find(|action| {
                        action.id == crate::quota_control_context::action_id("filters").unwrap()
                    })
                    .unwrap();
                let title = presentation.toolbar_label.unwrap();
                assert_eq!(title.contains("Group:"), expected != Ungrouped);
                if expected != Ungrouped {
                    assert!(title.contains(expected.label()), "{title}");
                }
            }
            assert_eq!(
                after_cycle, initial,
                "None restores the ungrouped Canvas at {width}x{height}"
            );

            let before_draft = after_cycle;
            for key in [
                KeyCode::F(3),
                KeyCode::Tab,
                KeyCode::Tab,
                KeyCode::Enter,
                KeyCode::Down,
                KeyCode::Enter,
            ] {
                input
                    .step(frames.as_mut(), provider_filter_key(key), |_| true)
                    .await;
            }
            assert_eq!(
                projected_group_facet(stack),
                Ungrouped,
                "picking a draft is not a commit"
            );
            assert_eq!(
                preferences.current_facet(),
                Ungrouped,
                "draft choices never save"
            );
            let cancelled = input
                .step(frames.as_mut(), provider_filter_key(KeyCode::Esc), |_| true)
                .await
                .to_string();
            assert_eq!(
                cancelled, before_draft,
                "cancel keeps committed grouping and focus at {width}x{height}"
            );

            for key in [
                KeyCode::F(3),
                KeyCode::Tab,
                KeyCode::Tab,
                KeyCode::Enter,
                KeyCode::Down,
                KeyCode::Enter,
                KeyCode::Tab,
                KeyCode::Enter,
            ] {
                input
                    .step(frames.as_mut(), provider_filter_key(key), |_| true)
                    .await;
            }
            assert_eq!(
                projected_group_facet(stack),
                Provider,
                "Apply commits the same facet at {width}x{height}"
            );
            assert_eq!(
                preferences.current_facet(),
                Provider,
                "Apply remembers the committed choice even after store failures"
            );
            sink.flush_and_check().unwrap();
        }
        preferences.finish();
        drop(preferences);
        tokio::time::timeout(std::time::Duration::from_secs(3), writer)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(store.0.lock().unwrap().last(), Some(&Provider));
    }
}

#[tokio::test]
async fn grouped_list_keeps_selection_skips_headings_and_captures_grouped_position_at_four_grids() {
    use codex_router_core::provider::Provider;
    use codex_router_quota_service::quota_display_facts::{QuotaAccountStatus, QuotaRowStatus};
    for (width, height) in [(120usize, 36usize), (76, 30), (48, 24), (48, 12)] {
        let mut model = quota_view_model();
        let base = model.rows.remove(0);
        model.rows = [
            (
                "r19-claude-a",
                Provider::Claude,
                QuotaAccountStatus::Available,
                2,
            ),
            ("r19-openai", Provider::Openai, QuotaAccountStatus::Held, 1),
            (
                "r19-claude-b",
                Provider::Claude,
                QuotaAccountStatus::Preferred,
                3,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (label, provider, status, clients))| {
            let mut row = base.clone();
            row.account_id = test_account_id(label);
            row.account = label.to_owned();
            row.details.account = label.to_owned();
            row.provider = provider;
            row.status = QuotaRowStatus::Account(status);
            row.active_clients_value = Some(clients);
            row.selected = index == 0;
            row
        })
        .collect();
        model.header = test_header_facts(3, 3);
        let identities = model
            .rows
            .iter()
            .map(|row| row.account_id.clone())
            .collect::<Vec<_>>();
        let controller = crate::test_support::QuotaFixtureController::default();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("grouped-input.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height,fixture_controller:Some(controller.clone()),dispatch_receipt:Some(sink.clone())));
        let bytes = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
        let frames = dispatch_render_loop(
            &mut component,
            MockTerminalConfig::with_events(events),
            bytes,
            writer,
        );
        tokio::pin!(frames);
        let mut input = QuotaInputFrameDriver::new(sender, path);
        current_quota_canvas(frames.as_mut()).await;
        let initial = controller.readback().unwrap();
        let mut targets = initial
            .surfaces
            .top()
            .focus
            .targets()
            .iter()
            .map(|target| target.id.as_str().to_owned())
            .collect::<Vec<_>>();
        targets.sort();
        assert_eq!(initial.browse_account, Some(identities[0].clone()));
        input
            .step(frames.as_mut(), provider_filter_key(KeyCode::F(6)), |_| {
                true
            })
            .await;
        assert_eq!(
            controller.readback().unwrap().browse_account,
            Some(identities[0].clone()),
            "regroup keeps account at {width}x{height}"
        );
        let top = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::Home), |_| {
                true
            })
            .await
            .to_string();
        assert!(
            top.contains("OpenAI") && top.contains("(1)") && top.contains("1 active client"),
            "provider heading at {width}x{height}:\n{top}"
        );
        assert_eq!(
            controller.readback().unwrap().browse_account,
            Some(identities[0].clone()),
            "ContentScroll Home keeps selection"
        );
        for (key, index) in [(KeyCode::Down, 2usize), (KeyCode::Up, 0), (KeyCode::Up, 1)] {
            let canvas = input
                .step(frames.as_mut(), provider_filter_key(key), |_| true)
                .await
                .to_string();
            let readback = controller.readback().unwrap();
            assert_eq!(
                readback.browse_account,
                Some(identities[index].clone()),
                "row-key navigation {key:?} at {width}x{height}"
            );
            assert_eq!(
                readback.list_selection.as_deref(),
                Some(identities[index].as_str())
            );
            assert!(
                canvas.contains(if index == 1 {
                    "r19-openai"
                } else if index == 0 {
                    "r19-claude-a"
                } else {
                    "r19-claude-b"
                }),
                "selected row revealed: {canvas}"
            );
            assert_eq!(
                readback.surfaces.top().focus.current(),
                Some(&crate::quota_list_controls::account_list_target().unwrap())
            );
        }
        input
            .step(frames.as_mut(), provider_filter_key(KeyCode::F(6)), |_| {
                true
            })
            .await;
        assert_eq!(
            controller.readback().unwrap().browse_account,
            Some(identities[1].clone()),
            "status regroup keeps account"
        );
        for index in [0usize, 2] {
            input
                .step(frames.as_mut(), provider_filter_key(KeyCode::Up), |_| true)
                .await;
            assert_eq!(
                controller.readback().unwrap().browse_account,
                Some(identities[index].clone())
            );
        }
        let grouped = controller.readback().unwrap();
        let mut regrouped_targets = grouped
            .surfaces
            .top()
            .focus
            .targets()
            .iter()
            .map(|target| target.id.as_str().to_owned())
            .collect::<Vec<_>>();
        regrouped_targets.sort();
        assert_eq!(
            regrouped_targets, targets,
            "headings add no focus targets; row targets follow grouped order"
        );
        input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Enter),
                |canvas| canvas.to_string().contains("Account details"),
            )
            .await;
        assert!(
            matches!(controller.readback().unwrap().captured,Some(crate::test_support::QuotaCapturedContext::Details(target)) if target.account_id==identities[2]&&target.position==0),
            "Details captures the grouped position"
        );
        sink.flush_and_check().unwrap();
    }
}
