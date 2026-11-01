//! Invocation results, one-flight admission and live captured Details through the real tree.
use super::*;
use crate::{
    QuotaOutcome, QuotaRefreshFailure, QuotaRefreshResult, QuotaRefresher, QuotaStatusViewOptions,
    test_support::{QuotaFixtureControl, QuotaFixtureOptions, quota_control_fixture},
};
use futures_util::StreamExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn quota_refresh_is_single_flight_and_reloads_captured_details_by_identity() {
    let base = quota_two_account_view_model();
    let alpha = base.rows[0].account_id.clone();
    let beta = base.rows[1].account_id.clone();
    for result in [
        QuotaRefreshResult::Refreshed,
        QuotaRefreshResult::Partial {
            failed: vec![QuotaRefreshFailure {
                account_id: beta.clone(),
                message: "beta provider unavailable".to_owned(),
            }],
        },
        QuotaRefreshResult::Failed {
            failed: vec![
                QuotaRefreshFailure {
                    account_id: alpha,
                    message: "alpha provider unavailable".to_owned(),
                },
                QuotaRefreshFailure {
                    account_id: beta.clone(),
                    message: "beta provider unavailable".to_owned(),
                },
            ],
            overall: None,
        },
    ] {
        run_refresh_scenario(base.clone(), beta.clone(), result).await;
    }
}

async fn run_refresh_scenario(
    initial: QuotaStatusViewModel,
    selected: codex_router_core::ids::AccountId,
    result: QuotaRefreshResult,
) {
    let mut fresh = initial.clone();
    fresh.rows.reverse();
    let beta = fresh
        .rows
        .iter_mut()
        .find(|row| row.account_id == selected)
        .unwrap();
    beta.details.weekly_window = "42% refreshed weekly observation".to_owned();
    let calls = Arc::new(AtomicUsize::new(0));
    let reloads = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Notify::new());
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let refresher: QuotaRefresher = {
        let calls = calls.clone();
        let release = release.clone();
        let result = result.clone();
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            let release = release.clone();
            let result = result.clone();
            started.send(()).unwrap();
            Box::pin(async move {
                release.notified().await;
                result
            })
        })
    };
    let loader: crate::QuotaStatusViewModelLoader = {
        let reloads = reloads.clone();
        Arc::new(move || {
            reloads.fetch_add(1, Ordering::SeqCst);
            let fresh = fresh.clone();
            Box::pin(async move { Some(fresh) })
        })
    };
    let mut fixture = quota_control_fixture(QuotaFixtureOptions {
        view_model: initial,
        ports: QuotaStatusViewOptions {
            reload_view_model: Some(loader),
            quota_refresher: Some(refresher),
            focused_account_id: Some(selected),
            notice: Some("Previous refresh result".to_owned()),
            ..Default::default()
        },
        width: 120,
        height: 36,
        control: QuotaFixtureControl::Browse,
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let captures = fixture
        .element
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(captures);
    let mut frames = Vec::new();
    let mut stage = 0;
    let mut started = false;
    let mut refreshing = false;
    let observation=tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            tokio::select! {
                entered=starts.recv(),if !started=>{entered.unwrap();started=true;}
                canvas=captures.next()=>{
                    let Some(canvas)=canvas else{break;};let frame=canvas.to_string();
                    if stage==0 {
                        assert!(frame.contains("❯ beta"),"the real tree starts on captured beta: {frame}");
                        sender.send(provider_filter_key(KeyCode::Char('r'))).unwrap();
                        sender.send(provider_filter_key(KeyCode::Char('r'))).unwrap();stage=1;
                    }
                    if stage==1&&frame.contains("Refreshing…"){
                        assert_eq!(frame.matches("Refreshing…").count(),1,"one in-flight status: {frame}");
                        assert!(!frame.contains("Previous refresh result"),"the in-flight status replaces the previous result: {frame}");
                        refreshing=true;
                    }
                    if stage==2&&frame.contains("Account details")&&frame.contains("beta") {
                        assert!(!frame.contains("42% refreshed"),"fresh data cannot appear before the held refresh completes");
                        release.notify_one();stage=3;
                    }else if stage==3&&frame.contains("42% refreshed weekly observation") {
                        assert!(frame.contains("beta"),"Details stays captured after row reorder: {frame}");
                        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                        sender.send(provider_filter_key(KeyCode::Char('q'))).unwrap();stage=4;
                    }
                    frames.push(frame);
                }
            }
            if stage==1&&started&&refreshing {
                assert_eq!(calls.load(Ordering::SeqCst),1,"two R presses in one burst start one refresh");
                assert_eq!(reloads.load(Ordering::SeqCst),0,"reload waits for service completion");
                sender.send(provider_filter_key(KeyCode::F(4))).unwrap();stage=2;
            }
        }
    }).await;
    release.notify_waiters();
    observation
        .unwrap_or_else(|_| panic!("refresh checkpoint not reached,stage={stage}: {frames:?}"));
    assert_eq!(stage, 4, "held refresh must reach fresh captured Details");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "no duplicate or automatic retry"
    );
    assert_eq!(
        reloads.load(Ordering::SeqCst),
        1,
        "one completion reload under the existing owner lock"
    );
    assert_eq!(*fixture.outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
    match result {
        QuotaRefreshResult::Refreshed => {
            assert!(frames.iter().any(|frame| frame.contains("Refreshed")))
        }
        QuotaRefreshResult::Partial { .. } => assert!(
            frames.iter().any(|frame| frame.contains("Partial refresh")
                && frame.contains("beta provider unavailable")),
            "{frames:?}"
        ),
        QuotaRefreshResult::Failed { .. } => assert!(
            frames.iter().any(|frame| frame.contains("Refresh failed")
                && frame.contains("alpha provider unavailable")
                && frame.contains("beta provider unavailable")),
            "{frames:?}"
        ),
    }
}
