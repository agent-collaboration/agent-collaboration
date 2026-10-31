//! Literal fixture consistency and error-notice chrome geometry on real components.
use super::quota_storyboard_fixture_test::{
    FixtureScenario, fixture_ports, fixture_scenario_model, fixture_snapshot,
};
use super::*;
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, ResetSessionIntent, ResetSessionPorts, ResetWorkflowSnapshot,
    WorkflowPhase,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct FixtureEffectObservation {
    calls: Arc<AtomicUsize>,
    stored: Arc<Mutex<QuotaStatusViewModel>>,
    initial: QuotaStatusViewModel,
}
impl FixtureEffectObservation {
    fn assert_unchanged(&self) {
        assert_eq!(
            self.calls.load(Ordering::SeqCst),
            0,
            "no fixture effect callbacks"
        );
        assert_eq!(*self.stored.lock().unwrap(), self.initial);
    }
}

fn full_fixture_component(
    scenario: FixtureScenario,
) -> (
    AnyElement<'static>,
    tokio::sync::mpsc::UnboundedReceiver<ResetSessionIntent>,
    tokio::sync::watch::Sender<ResetWorkflowSnapshot>,
    FixtureEffectObservation,
) {
    let model = fixture_scenario_model(scenario);
    let stored = Arc::new(Mutex::new(model.clone()));
    let (_release, receiver) = tokio::sync::watch::channel(false);
    let (floor, policy, refresh) = fixture_ports(stored.clone(), scenario, receiver);
    let calls = Arc::new(AtomicUsize::new(0));
    let floor: super::super::quota_floor_editor::WeeklyQuotaFloorSaver = {
        let calls = calls.clone();
        Arc::new(move |account, percent| {
            calls.fetch_add(1, Ordering::SeqCst);
            floor(account, percent)
        })
    };
    let policy: super::super::quota_account_options::CreditUsagePolicySaver = {
        let calls = calls.clone();
        Arc::new(move |account, generation, value| {
            calls.fetch_add(1, Ordering::SeqCst);
            policy(account, generation, value)
        })
    };
    let refresh: super::super::quota_account_options::CreditUsageRefresher = {
        let calls = calls.clone();
        Arc::new(move |account, generation| {
            calls.fetch_add(1, Ordering::SeqCst);
            refresh(account, generation)
        })
    };
    let observation = FixtureEffectObservation {
        calls: calls.clone(),
        stored: stored.clone(),
        initial: model.clone(),
    };
    let loader: crate::quota_status_view_model::QuotaStatusViewModelLoader = Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        let snapshot = stored.lock().unwrap().clone();
        Box::pin(async move { Some(snapshot) })
    });
    let (ports, intents, snapshots) = ResetSessionPorts::test_channels(fixture_snapshot(
        WorkflowPhase::Browse,
        ConfirmationSelection::No,
        scenario,
    ));
    let component=element!(QuotaStatusComponent(view_model:model,width:120usize,height:36usize,reload_view_model:Some(loader),weekly_floor_saver:Some(floor),credit_policy_saver:Some(policy),credit_usage_refresher:Some(refresh),reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver))).into_any();
    (component, intents, snapshots, observation)
}

#[tokio::test]
async fn normal_and_hard_notice_keep_bottom_border_and_footer_inside_120x36() {
    for scenario in [FixtureScenario::Normal, FixtureScenario::VisualHardCases] {
        let (mut component, mut intents, snapshots, effects) = full_fixture_component(scenario);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let canvas = tokio::time::timeout(
            Duration::from_secs(2),
            super::super::quota_frame_observation::next_current_frame(frames.as_mut()),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let frame = canvas.to_string();
        println!("CHROME ORIGINAL {scenario:?}120x36:\n{frame}");
        assert_eq!((canvas.width(), canvas.height()), (120, 36));
        let rows = frame.lines().collect::<Vec<_>>();
        let toolbar = rows
            .iter()
            .position(|row| row.contains("Menu F10") && row.contains("Details F4"))
            .unwrap();
        let body_top = rows.iter().position(|row| row.contains('┌')).unwrap();
        let body_bottom = rows.iter().rposition(|row| row.contains('└')).unwrap();
        println!(
            "CHROME GEOMETRY {scenario:?}:toolbar={toolbar},body_top={body_top},body_bottom={body_bottom},last={:?}",
            rows.last()
        );
        assert!(
            toolbar < body_top && body_top < body_bottom && body_bottom < 35,
            "toolbar and body inside outer border: {frame}"
        );
        assert!(
            rows[35].starts_with('╰') && rows[35].ends_with('╯'),
            "actual outer bottom border occupies final physical row: {frame}"
        );
        assert!(
            frame.contains("Quota · 2/2"),
            "compiled fixture's counts remain visible"
        );
        if scenario == FixtureScenario::VisualHardCases {
            assert!(
                rows.iter()
                    .take(body_top)
                    .any(|row| row.contains("Credentials locked")),
                "notice remains above browse body"
            );
        } else {
            assert!(!frame.contains("Credentials locked"));
        }
        sender
            .send(TerminalEvent::Key(KeyEvent::new(
                KeyEventKind::Press,
                KeyCode::F(10),
            )))
            .unwrap();
        let menu = tokio::time::timeout(
            Duration::from_secs(2),
            super::super::quota_frame_observation::next_current_frame(frames.as_mut()),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .to_string();
        assert!(
            menu.lines()
                .any(|line| line.contains("Floor") && line.contains("Ctrl+E")),
            "compiled fixture's available floor action stays reachable through Menu: {menu}"
        );
        assert!(matches!(
            intents.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(snapshots.borrow().phase(), WorkflowPhase::Browse);
        effects.assert_unchanged();
    }
}

#[tokio::test]
async fn hard_fixture_browse_and_captured_details_agree_on_literal_windows() {
    let (mut component, mut intents, snapshots, effects) =
        full_fixture_component(FixtureScenario::VisualHardCases);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let journey = async {
        let alpha = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        println!("WINDOW ORIGINAL ALPHA BROWSE:\n{alpha}");
        assert!(alpha.contains("92% · week · 6d 16h") && alpha.contains("83% · 5h · 2h 10m"));
        sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
        let frame = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        println!("WINDOW ORIGINAL ALPHA DETAILS:\n{frame}");
        let details = super::quota_ux_clarity_test::actual_dialog_text(&frame, "Account details");
        assert!(details.contains("alpha-tag") && details.contains("Alpha"));
        let weekly = details.lines().find(|row| row.contains("weekly")).unwrap();
        let short = details
            .lines()
            .find(|row| row.trim_start().starts_with("5h"))
            .unwrap();
        assert!(
            weekly.contains("92%") && weekly.contains("6d") && weekly.contains("16h"),
            "literal Alpha weekly: {details}"
        );
        assert!(
            short.contains("83%") && short.contains("2h") && short.contains("10m"),
            "literal Alpha short: {details}"
        );
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        assert_eq!(
            super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap()
                .to_string(),
            alpha
        );
        sender.send(provider_filter_key(KeyCode::Down)).unwrap();
        let beta = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        println!("WINDOW ORIGINAL BETA BROWSE:\n{beta}");
        assert!(beta.contains("75% · week") && !beta.contains("· —") && !beta.contains("No data"));
        sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
        let frame = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap()
            .to_string();
        println!("WINDOW ORIGINAL BETA DETAILS:\n{frame}");
        let details = super::quota_ux_clarity_test::actual_dialog_text(&frame, "Account details");
        assert!(details.contains("beta-tag") && details.contains("Beta"));
        let weekly = details.lines().find(|row| row.contains("weekly")).unwrap();
        let short = details
            .lines()
            .find(|row| row.trim_start().starts_with("5h"))
            .unwrap();
        assert!(
            weekly.contains("75%") && weekly.contains("reset unknown") && !weekly.contains("7d"),
            "literal Beta unknown-reset weekly: {details}"
        );
        assert!(
            short.contains("no data") && !short.contains('%'),
            "literal Beta missing short: {details}"
        );
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        assert_eq!(
            super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap()
                .to_string(),
            beta
        );
    };
    tokio::time::timeout(Duration::from_secs(3), journey)
        .await
        .unwrap();
    assert!(matches!(
        intents.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(snapshots.borrow().phase(), WorkflowPhase::Browse);
    effects.assert_unchanged();
}
