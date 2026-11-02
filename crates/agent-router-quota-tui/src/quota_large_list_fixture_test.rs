//! A separate immutable, effect-free list seed for the four-grid navigation proof.
use codex_router_core::ids::AccountId;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;

pub(super) const LARGE_LIST_ACCOUNTS: [(&str, &str); 60] = [
    ("quota-nav-00", "00 Account"),
    ("quota-nav-01", "01 Account"),
    ("quota-nav-02", "02 Account"),
    ("quota-nav-03", "03 Account"),
    ("quota-nav-04", "04 Account"),
    ("quota-nav-05", "05 Account"),
    ("quota-nav-06", "06 Account"),
    ("quota-nav-07", "07 Account"),
    ("quota-nav-08", "08 Account"),
    ("quota-nav-09", "09 Account"),
    ("quota-nav-10", "10 Account"),
    ("quota-nav-11", "11 Account"),
    ("quota-nav-12", "12 Account"),
    ("quota-nav-13", "13 Account"),
    ("quota-nav-14", "14 Account"),
    ("quota-nav-15", "15 Account"),
    ("quota-nav-16", "16 Account"),
    ("quota-nav-17", "17 Account"),
    ("quota-nav-18", "18 Account"),
    ("quota-nav-19", "19 Account"),
    ("quota-nav-20", "20 Account"),
    ("quota-nav-21", "21 Account"),
    ("quota-nav-22", "22 Account"),
    ("quota-nav-23", "23 Account"),
    ("quota-nav-24", "24 Account"),
    ("quota-nav-25", "25 Account"),
    ("quota-nav-26", "26 Account"),
    ("quota-nav-27", "27 Account"),
    ("quota-nav-28", "28 Account"),
    ("quota-nav-29", "29 Account"),
    ("quota-nav-30", "30 Account"),
    ("quota-nav-31", "31 Account"),
    ("quota-nav-32", "32 Account"),
    ("quota-nav-33", "33 Account"),
    ("quota-nav-34", "34 Account"),
    ("quota-nav-35", "35 Account"),
    ("quota-nav-36", "36 Account"),
    ("quota-nav-37", "37 Account"),
    ("quota-nav-38", "38 Account"),
    ("quota-nav-39", "39 Account"),
    ("quota-nav-40", "40 Account"),
    ("quota-nav-41", "41 Account"),
    ("quota-nav-42", "42 Account"),
    ("quota-nav-43", "43 Account"),
    ("quota-nav-44", "44 Account"),
    ("quota-nav-45", "45 Account"),
    ("quota-nav-46", "46 Account"),
    ("quota-nav-47", "47 Account"),
    ("quota-nav-48", "48 Account"),
    ("quota-nav-49", "49 Account"),
    ("quota-nav-50", "50 Account"),
    ("quota-nav-51", "51 Account"),
    ("quota-nav-52", "52 Account"),
    ("quota-nav-53", "53 Account"),
    ("quota-nav-54", "54 Account"),
    ("quota-nav-55", "55 Account"),
    ("quota-nav-56", "56 Account"),
    ("quota-nav-57", "57 Account"),
    ("quota-nav-58", "58 Account"),
    ("quota-nav-59", "59 Account"),
];

pub(super) fn with_large_list_accounts(mut model: QuotaStatusViewModel) -> QuotaStatusViewModel {
    let template = model
        .rows
        .first()
        .expect("synthetic account template")
        .clone();
    model.rows = LARGE_LIST_ACCOUNTS
        .iter()
        .enumerate()
        .map(|(index, (id, label))| {
            let mut row = template.clone();
            row.account_id = AccountId::new(*id).unwrap();
            row.account = (*label).to_owned();
            row.details.account = (*label).to_owned();
            row.selected = index == 0;
            row
        })
        .collect();
    model.header.pool.usable = 60;
    model.header.pool.reserve = 0;
    model.header.pool.blocked = 0;
    model.header.pool.unknown = 0;
    model.header.pool.excluded = 0;
    model.header.freshness.fresh = 60;
    model.header.freshness.stale = 0;
    model.header.freshness.unknown = 0;
    model
}

#[test]
fn large_list_seed_has_sixty_fixed_unique_ids_labels_and_order() {
    let model = super::fixture_scenario_model(super::FixtureScenario::LargeList);
    assert_eq!(model.rows.len(), 60);
    let metadata = serde_json::json!({"scenario":"large-list","navigationPolicy":"ContentScroll","effectPorts":[],"observedClients":model.header.observed_clients,
        "accounts":model.rows.iter().map(|row|serde_json::json!({"accountId":row.account_id.as_str(),"label":row.account,"provider":row.provider.as_str(),"enabled":row.enabled,"selected":row.selected,"accountTag":row.account_tag,"credentialGeneration":row.active_credential_generation,"weeklyFloorPercent":row.weekly_quota_floor_percent,"weeklyWindow":row.weekly_window,"shortWindow":row.short_window,"status":format!("{:?}",row.status),"activeClients":row.active_clients,"activeClientsValue":row.active_clients_value,"resetCredits":row.reset_credits,"creditUsageSummary":row.credit_usage_summary})).collect::<Vec<_>>()});
    println!("QUOTA_LARGE_LIST_SEED {metadata}");
    let actual = model
        .rows
        .iter()
        .map(|row| (row.account_id.as_str(), row.account.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(actual, LARGE_LIST_ACCOUNTS);
    assert_eq!(
        actual
            .iter()
            .map(|row| row.0)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        60
    );
    assert_eq!(
        actual
            .iter()
            .map(|row| row.1)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        60
    );
    assert_eq!(actual.first(), Some(&("quota-nav-00", "00 Account")));
    assert_eq!(actual.last(), Some(&("quota-nav-59", "59 Account")));
    assert!(
        model
            .rows
            .iter()
            .enumerate()
            .all(|(index, row)| row.selected == (index == 0))
    );
}

#[tokio::test]
async fn large_list_has_more_than_two_real_navigation_pages_at_every_grid() {
    use crate::{
        quota_fixture_controller::QuotaFixtureController,
        quota_status_component::QuotaStatusComponent,
        quota_test_support::{
            QuotaInputFrameDriver, current_quota_canvas,
            frame_output::{SharedFrameOutput, dispatch_render_loop},
        },
    };
    use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
    use futures_util::StreamExt;
    use iocraft::prelude::*;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    for (width, height) in [(120usize, 36usize), (76, 30), (48, 12), (32, 24)] {
        tokio::time::timeout(Duration::from_secs(5),async {
            // Arrange: no loader, saver, refresher, account commands or reset ports.
            let model=super::fixture_scenario_model(super::FixtureScenario::LargeList);
            let directory=tempfile::tempdir().unwrap();
            let receipt=directory.path().join("large-list-input.jsonl");
            let sink=InputDispatchReceiptSink::create(&receipt).unwrap();
            let controller=QuotaFixtureController::default();
            let outcome=Arc::new(Mutex::new(None));
            let (sender,receiver)=tokio::sync::mpsc::unbounded_channel();
            let events=futures_util::stream::unfold(receiver,|mut receiver|async {receiver.recv().await.map(|event|(event,receiver))});
            let mut component=element!(QuotaStatusComponent(view_model:model,width,height,dispatch_receipt:Some(sink.clone()),fixture_controller:Some(controller.clone()),outcome_sink:Some(outcome.clone()),spinner_interval:Duration::from_secs(3600)));
            let bytes=SharedFrameOutput::default();
            let writer=Box::new(sink.frame_writer(bytes.clone()).unwrap());
            let frames=dispatch_render_loop(&mut component,MockTerminalConfig::with_events(events),bytes,writer);
            tokio::pin!(frames);
            let initial = current_quota_canvas(frames.as_mut()).await.to_string();
            let mut input=QuotaInputFrameDriver::new(sender.clone(),receipt);
            let key=|code|TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press,code));
            let selected=||controller.readback().unwrap().list_selection.unwrap();
            assert_eq!(selected(),"quota-nav-00");
            // Act: two actual page moves still leave another page to navigate.
            let mut previous=initial.clone();
            for _ in 0..2 {
                let page=input.step(frames.as_mut(),key(KeyCode::PageDown),|_|true).await.to_string();
                assert_ne!(page,previous,"PageDown must actually scroll content at {width}x{height}");
                assert_eq!(selected(),"quota-nav-00","ContentScroll preserves account selection");
                previous=page;
            }
            let end=input.step(frames.as_mut(),key(KeyCode::End),|_|true).await.to_string();
            assert_ne!(end,previous,"sixty rows must extend beyond two viewport pages at {width}x{height}");
            assert_eq!(selected(),"quota-nav-00");
            let home=input.step(frames.as_mut(),key(KeyCode::Home),|_|true).await.to_string();
            assert_eq!(home,initial,"Home restores the actual initial list pose");
            input.step(frames.as_mut(),key(KeyCode::Down),|_|true).await;
            assert_eq!(selected(),"quota-nav-01","Arrow Down selects the next literal account");
            input.step(frames.as_mut(),key(KeyCode::Up),|_|true).await;
            assert_eq!(selected(),"quota-nav-00");
            // Assert: the literal source order survives actual scrolling and teardown.
            let readback=controller.readback().unwrap();
            assert_eq!(readback.view_model.rows.iter().map(|row|(row.account_id.as_str(),row.account.as_str())).collect::<Vec<_>>(),LARGE_LIST_ACCOUNTS);
            sender.send(key(KeyCode::Char('q'))).unwrap();
            while frames.next().await.is_some() {}
            assert_eq!(*outcome.lock().unwrap(),Some(crate::QuotaOutcome::Quit));
        }).await.expect("large-list navigation and real Quit must complete");
    }
}
