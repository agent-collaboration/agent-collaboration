// R4: an empty main-screen Esc is inert; the next action still reaches the screen.
#[tokio::test]
async fn quota_cutover_empty_escape_does_not_reach_the_quit_boundary() {
    for (code, expected_outcome) in [(KeyCode::Esc, None), (KeyCode::Char('q'), Some(crate::QuotaOutcome::Quit))] {
        let quit_count = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&quit_count);
        let outcomes=Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded=outcomes.clone();
        let (sender,receiver)=tokio::sync::mpsc::unbounded_channel();
        let quit_sender=sender.clone();
        let mut component = element!(QuotaStatusComponent(
            view_model: quota_two_account_view_model(), width:120usize, height:36usize,
            quit_observer:Some(Handler::from(move |()| { observed.fetch_add(1,Ordering::SeqCst); })),
            routed_outcome_observer:Some(Handler::from(move |outcome| {
                let mut outcomes=recorded.lock().unwrap();
                outcomes.push(outcome);
                if outcomes.len()==1 && code==KeyCode::Esc {quit_sender.send(provider_filter_key(KeyCode::Char('q'))).unwrap();}
            }))
        ));
        sender.send(provider_filter_key(code)).unwrap();
        let events=futures_util::stream::unfold(receiver,|mut receiver|async {receiver.recv().await.map(|event|(event,receiver))});
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(
            events
        )).collect::<Vec<_>>().await;
        assert!(!frames.is_empty());
        assert_eq!(outcomes.lock().unwrap().first(),Some(&expected_outcome),"event-time quota outcome for {code:?}");
        assert_eq!(quit_count.load(Ordering::SeqCst), 1,
            "read back the application's own quit boundary for {code:?}");
    }
}

#[tokio::test]
async fn quota_cutover_details_exposes_the_captured_accounts_actions() {
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:120usize,height:36usize));
    let captures = component.mock_terminal_render_loop(MockTerminalConfig::with_events(
        futures_util::stream::iter([provider_filter_key(KeyCode::F(4))])
    ));
    tokio::pin!(captures);
    let mut frames = Vec::new();
    while let Some(canvas) = captures.next().await {
        let frame = canvas.to_string();
        let entered = frame.contains("Account details");
        frames.push(frame);
        if entered { break; }
    }
    assert!(frames.iter().any(|frame| frame.contains("Account details")
        && frame.contains("alpha") && frame.contains("Actions") && frame.contains("Close Esc")),
        "Details must expose the captured account's actions: {frames:?}");
}

// R13: actions remain reachable from the top toolbar/menu, including narrow layouts.
#[tokio::test]
async fn quota_cutover_browse_exposes_the_menu_instead_of_a_bottom_action_row() {
    let frame = render_quota_capture_model_at(
        quota_two_account_view_model(),
        120,
        36,
        vec![provider_filter_key(KeyCode::Char('q'))],
    )
    .await;
    assert!(frame.contains("≡") && frame.contains("Menu") && frame.contains("F10"), "{frame}");
    let rows = frame.lines().collect::<Vec<_>>();
    assert!(
        !rows.iter().rev().take(4).any(|row| row.contains("F4 Details") || row.contains("Ctrl+R Options")),
        "the browse screen has no bottom row of actions: {frame}"
    );
}

// R13/R17/R18: F10 reaches the real action scope, not a separately typed help list.
#[tokio::test]
async fn quota_cutover_f10_exposes_account_actions_and_refresh() {
    let (sender,receiver)=tokio::sync::mpsc::unbounded_channel();
    let events=futures_util::stream::unfold(receiver,|mut receiver|async {receiver.recv().await.map(|event|(event,receiver))});
    let mut component=element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:120usize,height:36usize));
    let captures=component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(captures);
    let mut frames=Vec::new();let mut opened=false;
    tokio::time::timeout(Duration::from_secs(2),async {
        while let Some(canvas)=captures.next().await {
            let frame=canvas.to_string();let reached=frame.contains("Log in again");frames.push(frame);
            if reached {break;}
            if !opened {sender.send(provider_filter_key(KeyCode::F(10))).unwrap();opened=true;}
        }
    }).await.expect("F10 must reach the menu checkpoint");
    assert!(
        frames.iter().any(|frame| ["Details", "Filters", "Pool", "Options", "Help", "Quit", "Log in again", "Refresh"].iter().all(|action| frame.contains(action))),
        "F10 must expose every quota action, including login and refresh: {frames:?}"
    );
}
