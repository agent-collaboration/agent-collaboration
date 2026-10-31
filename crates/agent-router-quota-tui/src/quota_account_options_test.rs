use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use iocraft::prelude::*;

use codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase;

use super::super::quota_browse_presentation_test::quota_two_account_view_model;
use super::super::quota_status_component::QuotaStatusComponent;
use super::rendering::AccountOptionsPanelProps;
use super::rendering::account_options_message_text;
use super::rendering::render_account_options_panel;
use super::*;

#[test]
fn account_options_render_explicit_tabs_and_unchecked_observation() {
    let view_model = quota_two_account_view_model();
    let mut options = AccountOptionsState::new(&view_model.rows[0], 1);
    options.tab = AccountOptionsTab::Credits;
    assert_eq!(
        account_options_footer(&options),
        "tab resets  enter edit  r refresh  esc back"
    );

    let frame = render_account_options_panel(AccountOptionsPanelProps {
        options: &options,
        reset_snapshot: None,
        reset_target: None,
        width: 100,
        height: 24,
        inventory_page_start: 0,
        spinner_tick: 0,
    })
    .render(None)
    .to_string();

    assert!(frame.contains("Resets  [ Credits ]"), "{frame}");
    assert!(frame.contains("Credit balance"), "{frame}");
    assert!(frame.contains("Credit usage"), "{frame}");
    assert!(frame.contains("not checked"), "{frame}");
    assert!(!frame.contains("checked unknown ago"), "{frame}");

    options.tab = AccountOptionsTab::Resets;
    assert_eq!(
        account_options_footer(&options),
        "enter inspect  tab credits  esc back"
    );
    let reset_frame = render_account_options_panel(AccountOptionsPanelProps {
        options: &options,
        reset_snapshot: None,
        reset_target: None,
        width: 100,
        height: 24,
        inventory_page_start: 0,
        spinner_tick: 0,
    })
    .render(None)
    .to_string();
    assert!(reset_frame.contains("[ Resets ]  Credits"), "{reset_frame}");
}

#[test]
fn account_options_inventory_pages_render_every_voucher_once() {
    use codex_router_quota_service::quota_reset::reset_session_supervisor::ConfirmationSelection;
    use codex_router_quota_service::quota_reset::reset_session_supervisor::LiveWeeklyDisplayFacts;
    use codex_router_quota_service::quota_reset::reset_session_supervisor::ResetCreditDisplayRecord;
    use codex_router_quota_service::quota_reset::reset_session_supervisor::ResetCreditDisplayStatusDto;
    use codex_router_quota_service::quota_reset::reset_session_supervisor::ResetValueProvenance;
    use codex_router_quota_service::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot;
    use codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase;

    let view_model = quota_two_account_view_model();
    let options = AccountOptionsState::new(&view_model.rows[0], 1);
    let target = super::super::quota_reset_presentation_model::ResetPaneTarget {
        account_id: view_model.rows[0].account_id.clone(),
        active_credential_generation: 7,
        account_label: view_model.rows[0].account.clone(),
        account_tag: view_model.rows[0].account_tag.clone(),
        saved_reset_credits: "5".to_owned(),
        saved_weekly_window: "weekly".to_owned(),
    };
    let inventory = (1..=5)
        .map(|index| ResetCreditDisplayRecord {
            id_hint: format!("voucher-{index}"),
            status: ResetCreditDisplayStatusDto::Available,
            title: Some(format!("Voucher {index}")),
            expires_unix_seconds: Some(1_900_000_000 + i64::from(index)),
            earliest_usable: index == 1,
        })
        .collect();
    let snapshot = ResetWorkflowSnapshot::test_snapshot(
        WorkflowPhase::Inspected,
        ConfirmationSelection::No,
        Default::default(),
        None,
        Some(LiveWeeklyDisplayFacts {
            remaining_percent: 4,
            provenance: ResetValueProvenance::CurrentLive,
        }),
        inventory,
        None,
    );
    let outer_height = account_options_content_height(&options, Some(&snapshot), Some(&target), 0);
    let body_height = outer_height.saturating_sub(7);
    let page_size = account_options_reset_inventory_page_size(body_height);

    let mut page_start = 0;
    let mut visited = Vec::new();
    loop {
        let frame = render_account_options_panel(AccountOptionsPanelProps {
            options: &options,
            reset_snapshot: Some(&snapshot),
            reset_target: Some(&target),
            width: 100,
            height: outer_height,
            inventory_page_start: page_start,
            spinner_tick: 0,
        })
        .render(None)
        .to_string();
        if page_start == 0 {
            assert!(frame.contains("1-4 of 5"), "{frame}");
        } else {
            assert_eq!(page_start, 4);
            assert!(frame.contains("5-5 of 5"), "{frame}");
        }
        let page_end = page_start.saturating_add(page_size).min(5);
        for voucher_number in page_start + 1..=page_end {
            assert!(
                frame.contains(&format!("[voucher-{voucher_number}]")),
                "page start {page_start} omitted voucher {voucher_number}:\n{frame}"
            );
            visited.push(voucher_number);
        }
        let next_page = super::super::quota_reset_presentation_model::credit_page_start(
            page_start, 5, page_size, true,
        );
        if next_page == page_start {
            break;
        }
        page_start = next_page;
    }

    assert_eq!(page_size, 4);
    assert_eq!(visited, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn control_c_in_credit_browsing_exits_the_component() {
    assert_credit_control_c_exits_the_component(false).await;
}

#[tokio::test]
async fn control_c_in_credit_editor_exits_the_component() {
    assert_credit_control_c_exits_the_component(true).await;
}

#[test]
fn protected_reset_phases_keep_forced_keys_with_the_reset_owner() {
    let view_model = quota_two_account_view_model();
    let mut options = AccountOptionsState::new(&view_model.rows[0], 1);
    options.tab = AccountOptionsTab::Credits;

    for phase in [
        WorkflowPhase::Confirming,
        WorkflowPhase::Revalidating,
        WorkflowPhase::Committing,
        WorkflowPhase::Result,
    ] {
        for (code, modifiers) in [
            (KeyCode::Char('c'), KeyModifiers::CONTROL),
            (KeyCode::Char('d'), KeyModifiers::CONTROL),
        ] {
            let credit_action = account_options_key_action(&options, phase, code, modifiers);
            assert_eq!(credit_action, AccountOptionsKeyAction::Ignore, "{phase:?}");
            assert_eq!(
                update_credit_policy_editor(&mut options, credit_action, None),
                None,
                "{phase:?} input must not emit a credit mutation"
            );

            options.tab = AccountOptionsTab::Resets;
            assert_eq!(
                account_options_key_action(&options, phase, code, modifiers),
                AccountOptionsKeyAction::ResetWorkflow,
                "the reset workflow retains control during {phase:?}"
            );
            let reset_action = super::super::quota_reset_keyboard_interaction::reset_key_action(
                phase,
                codex_router_quota_service::quota_reset::reset_session_supervisor::ConfirmationSelection::No,
                false,
                code,
                modifiers,
            );
            if phase == WorkflowPhase::Committing {
                assert_eq!(
                    reset_action,
                    super::super::quota_reset_keyboard_interaction::ResetKeyAction::None,
                    "Committing cannot be canceled or force-exited by a key"
                );
            } else {
                assert_eq!(
                    reset_action,
                    super::super::quota_reset_keyboard_interaction::ResetKeyAction::ExitPrecommit,
                    "{phase:?} keeps its existing reset shutdown path"
                );
            }
            options.tab = AccountOptionsTab::Credits;
        }
    }
}

#[test]
fn reset_review_and_consume_phases_do_not_offer_tab_switches() {
    let view_model = quota_two_account_view_model();
    let options = AccountOptionsState::new(&view_model.rows[0], 1);
    for phase in [
        WorkflowPhase::Confirming,
        WorkflowPhase::Revalidating,
        WorkflowPhase::Committing,
        WorkflowPhase::Result,
    ] {
        let action = account_options_key_action(
            &options,
            phase,
            iocraft::prelude::KeyCode::Tab,
            KeyModifiers::empty(),
        );
        assert_ne!(
            action,
            AccountOptionsKeyAction::SwitchTab(AccountOptionsTab::Credits),
            "tab switching must remain unavailable during {phase:?}"
        );
    }
}

#[test]
fn pending_tab_transition_keeps_escape_back_and_truthful_shortcuts() {
    let view_model = quota_two_account_view_model();
    let mut options = AccountOptionsState::new(&view_model.rows[0], 1);
    options.tab = AccountOptionsTab::Credits;
    options.pending_tab = Some(AccountOptionsTab::Resets);
    options.pending_tab_request = Some(InspectionTabRequestId::new(1, 2));

    assert_eq!(
        account_options_key_action(
            &options,
            WorkflowPhase::Browse,
            KeyCode::Esc,
            KeyModifiers::empty(),
        ),
        AccountOptionsKeyAction::Close,
        "Esc should close the pane while a tab acknowledgement waits behind refresh"
    );
    assert_eq!(
        account_options_key_action(
            &options,
            WorkflowPhase::Browse,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ),
        AccountOptionsKeyAction::ExitPending,
        "forced exit should reach the component while a tab acknowledgement is pending"
    );
    assert_eq!(
        account_options_key_action(
            &options,
            WorkflowPhase::Inspecting,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ),
        AccountOptionsKeyAction::ResetWorkflow,
        "inspection exit continues through the existing reset shutdown path"
    );
    options.tab = AccountOptionsTab::Resets;
    options.pending_tab = Some(AccountOptionsTab::Credits);
    assert_eq!(
        account_options_key_action(
            &options,
            WorkflowPhase::Confirming,
            KeyCode::Enter,
            KeyModifiers::empty(),
        ),
        AccountOptionsKeyAction::ResetWorkflow,
        "a protected reset confirmation remains owned by the reset workflow"
    );
    assert_eq!(
        account_options_footer(&options),
        "tab change pending  esc back  q/ctrl-c exit"
    );
}

#[test]
fn reset_inspection_footer_lists_account_options_and_review_keys() {
    let inspecting_footer =
        super::rendering::account_options_inspection_footer(WorkflowPhase::Inspecting, 100)
            .expect("Inspecting uses account-options navigation");
    assert!(inspecting_footer.contains("tab/←/→ credits"));
    assert!(inspecting_footer.contains("esc/ctrl-r back"));

    let inspected_footer =
        super::rendering::account_options_inspection_footer(WorkflowPhase::Inspected, 100)
            .expect("Inspected keeps reset review controls alongside account-options navigation");
    for marker in [
        "tab/←/→ credits",
        "enter review",
        "pgup/pgdn pages",
        "esc/ctrl-r back",
    ] {
        assert!(
            inspected_footer.contains(marker),
            "missing {marker} in {inspected_footer}"
        );
    }

    let narrow_footer =
        super::rendering::account_options_inspection_footer(WorkflowPhase::Inspected, 48)
            .expect("narrow Inspected footer keeps its primary actions");
    for marker in ["tab/←/→ credits", "enter review", "esc/ctrl-r back"] {
        assert!(
            narrow_footer.contains(marker),
            "missing {marker} in {narrow_footer}"
        );
    }

    for protected_phase in [
        WorkflowPhase::Confirming,
        WorkflowPhase::Revalidating,
        WorkflowPhase::Committing,
        WorkflowPhase::Result,
    ] {
        assert_eq!(
            super::rendering::account_options_inspection_footer(protected_phase, 100),
            None,
            "protected phase {protected_phase:?} retains its established reset footer"
        );
    }
}

#[tokio::test]
async fn ctrl_r_opens_disabled_credentialless_account_and_switches_to_credits() {
    let mut view_model = quota_two_account_view_model();
    view_model.rows[0].enabled = false;
    view_model.rows[0].active_credential_generation = None;
    let events = vec![
        (
            TerminalEvent::Key(control_key('r')),
            Some("Resets are unavailable"),
        ),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
            None,
        ),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Right)),
            None,
        ),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
            Some("Credit usage"),
        ),
        (TerminalEvent::Key(control_key('c')), None),
    ];

    let frames = render_account_options_component_frames(view_model, 100, 36, events, None).await;

    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("Resets are unavailable while this account is disabled."))
    );
    assert!(frames.iter().any(|frame| frame.contains("Close")));
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("Options · Credits"))
    );
    assert!(frames.iter().any(|frame| frame.contains("Credit usage")));
}

#[tokio::test]
async fn account_options_panel_keeps_spare_height_outside_its_border() {
    let events = vec![
        (TerminalEvent::Key(control_key('r')), Some("Inspect")),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
            None,
        ),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Right)),
            None,
        ),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
            Some("Credit usage"),
        ),
        (TerminalEvent::Key(control_key('c')), None),
    ];
    let frames = render_account_options_component_frames(
        quota_two_account_view_model(),
        100,
        36,
        events,
        None,
    )
    .await;
    let frame = frames
        .iter()
        .rev()
        .find(|frame| frame.contains("Options · Credits") && frame.contains("Credit usage"))
        .expect("Credits pane should render");
    let lines = frame.lines().collect::<Vec<_>>();
    let title = lines
        .iter()
        .position(|line| line.contains("Options · Credits"))
        .unwrap();
    let top = (0..title)
        .rev()
        .find(|&row| lines[row].contains('╭'))
        .expect("actual content frame top");
    let left = unicode_width::UnicodeWidthStr::width(lines[top].split_once('╭').unwrap().0);
    let bottom = (title + 1..lines.len())
        .find(|&row| {
            lines[row].find('╰').is_some_and(|byte| {
                unicode_width::UnicodeWidthStr::width(lines[row].get(..byte).unwrap()) == left
            })
        })
        .expect("same actual frame bottom");
    let actions = lines
        .iter()
        .position(|line| line.contains("Edit") && line.contains("Close"))
        .expect("fixed actions");
    assert!(
        top > 0 && bottom < 35 && bottom - top < 24 && actions > title && actions < bottom,
        "bounded content frame leaves spare outer height and keeps actions inside: {frame}"
    );
    let scoped = crate::quota_browse_presentation_test::quota_ux_clarity_test::actual_dialog_text(
        frame,
        "Options · Credits",
    );
    let words = scoped.split_whitespace().collect::<Vec<_>>().join(" ");
    for fact in [
        "Credit balance",
        "Status Unknown",
        "Freshness Unknown",
        "Observation not checked",
        "Credit usage",
        "Use credits Disallow",
        "Edit",
        "Refresh",
        "Close",
    ] {
        assert!(
            words.contains(fact),
            "{fact} is inside the bounded content frame: {scoped}"
        );
    }
}

include!("quota_account_options_input_test.rs");

#[cfg(test)]
#[path = "quota_account_options_paging_test.rs"]
mod quota_account_options_paging_test;

#[path = "quota_credit_presentation_test.rs"]
mod quota_credit_presentation_test;
