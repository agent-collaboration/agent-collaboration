//! Captured account actions pass through the actual shared input owner and dialog tree.
use super::{provider_filter_key, quota_two_account_view_model};
use crate::{
    AccountActionTarget, QuotaAccountCommands, QuotaOutcome, QuotaRefreshResult, QuotaRefresher,
    QuotaStatusViewOptions, quota_account_command_runner,
    test_support::{QuotaFixtureControl, QuotaFixtureOptions, quota_control_fixture},
};
use codex_router_quota_service::quota_display_model::QuotaLoginState;
use futures_util::{Stream, StreamExt};
use iocraft::Canvas;
use iocraft::prelude::*;
use std::time::Duration;
use std::{pin::Pin, sync::Arc};

fn control_event(code: KeyCode) -> TerminalEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, code);
    key.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}

async fn account_checkpoint<TFrames>(frames: Pin<&mut TFrames>, marker: &str) -> Canvas
where
    TFrames: Stream<Item = Canvas> + ?Sized,
{
    crate::quota_frame_observation::next_matching_frame(frames, Duration::from_secs(2), |canvas| {
        canvas.to_string().contains(marker)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn details_login_is_primary_only_when_the_captured_account_requires_login() {
    for login_state in [QuotaLoginState::NotRequired, QuotaLoginState::Required] {
        let mut model = quota_two_account_view_model();
        model.rows[1].login_state = login_state;
        let target = AccountActionTarget::from_row(&model.rows[1], 1).unwrap();
        let mut fixture = quota_control_fixture(QuotaFixtureOptions {
            view_model: model,
            ports: QuotaStatusViewOptions {
                focused_account_id: Some(target.account_id.clone()),
                ..Default::default()
            },
            width: 120,
            height: 36,
            control: QuotaFixtureControl::Details,
        });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
            &mut fixture.element,
            MockTerminalConfig::with_events(events),
        );
        tokio::pin!(frames);
        let entered = account_checkpoint(frames.as_mut(), "Account details").await;
        let frame = entered.to_string();
        assert!(frame.contains("beta"));
        // Lead taste review 01a18f46: healthy Details keeps login in Actions only.
        let buttons = frame
            .lines()
            .find(|line| line.contains("Actions ▾"))
            .unwrap();
        assert_eq!(
            buttons.contains("Log in again"),
            login_state == QuotaLoginState::Required,
            "{frame}"
        );
        assert!(
            buttons.contains(if login_state == QuotaLoginState::Required {
                "● Log in again"
            } else {
                "● Close Esc"
            }),
            "{frame}"
        );
        sender.send(control_event(KeyCode::Enter)).unwrap();
        if login_state == QuotaLoginState::NotRequired {
            let returned = account_checkpoint(frames.as_mut(), "Quota ·").await;
            assert!(!returned.to_string().contains("Account details"));
            assert_eq!(*fixture.outcome.lock().unwrap(), None);
            sender
                .send(provider_filter_key(KeyCode::Char('q')))
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while frames.next().await.is_some() {}
        })
        .await
        .unwrap();
        let expected = if login_state == QuotaLoginState::Required {
            QuotaOutcome::LogInAgain { target }
        } else {
            QuotaOutcome::Quit
        };
        assert_eq!(*fixture.outcome.lock().unwrap(), Some(expected));
    }
}

#[tokio::test]
async fn details_login_retains_opened_metadata_after_refresh_and_never_retargets_a_missing_id() {
    for missing in [false, true] {
        let mut model = quota_two_account_view_model();
        model.rows[1].login_state = QuotaLoginState::Required;
        let target = AccountActionTarget::from_row(&model.rows[1], 1).unwrap();
        let mut fresh = model.clone();
        if missing {
            fresh.rows.remove(1);
        } else {
            fresh.rows[1].account = "renamed beta".to_owned();
            fresh.rows[1].details.account = "renamed beta".to_owned();
            fresh.rows[1].details.weekly_window = "42% changed observation".to_owned();
            fresh.rows[1].provider = codex_router_core::provider::Provider::Claude;
            fresh.rows.reverse();
        }
        let release = Arc::new(tokio::sync::Notify::new());
        let held = release.clone();
        let refresher: QuotaRefresher = Arc::new(move || {
            let held = held.clone();
            Box::pin(async move {
                held.notified().await;
                QuotaRefreshResult::Refreshed
            })
        });
        let loader: crate::QuotaStatusViewModelLoader = Arc::new(move || {
            let fresh = fresh.clone();
            Box::pin(async move { Some(fresh) })
        });
        let mut fixture = quota_control_fixture(QuotaFixtureOptions {
            view_model: model,
            ports: QuotaStatusViewOptions {
                focused_account_id: Some(target.account_id.clone()),
                quota_refresher: Some(refresher),
                reload_view_model: Some(loader),
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
        let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
            &mut fixture.element,
            MockTerminalConfig::with_events(events),
        );
        tokio::pin!(frames);
        account_checkpoint(frames.as_mut(), "❯ beta").await;
        sender
            .send(provider_filter_key(KeyCode::Char('r')))
            .unwrap();
        account_checkpoint(frames.as_mut(), "Refreshing…").await;
        sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
        let details = account_checkpoint(frames.as_mut(), "Account details").await;
        assert!(details.to_string().contains("beta"));
        release.notify_one();
        let marker = if missing {
            "Account is no longer listed"
        } else {
            "42% changed observation"
        };
        let updated = account_checkpoint(frames.as_mut(), marker).await;
        assert!(updated.to_string().contains("Account details"));
        if !missing {
            let frame = updated.to_string();
            let header = frame
                .lines()
                .find(|line| line.contains("Account details"))
                .unwrap();
            assert!(
                header.contains("beta [beta-tag]") && !header.contains("renamed beta"),
                "captured dialog identity survives refresh: {frame}"
            );
            assert!(
                frame.contains("codex-router account login --provider openai --label beta"),
                "preview retains captured metadata: {frame}"
            );
            assert!(!frame.contains("--provider claude"), "{frame}");
        }
        sender.send(control_event(KeyCode::Enter)).unwrap();
        if missing {
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            let returned = account_checkpoint(frames.as_mut(), "Quota ·").await;
            assert!(!returned.to_string().contains("Account details"));
            assert_eq!(*fixture.outcome.lock().unwrap(), None);
            sender
                .send(provider_filter_key(KeyCode::Char('q')))
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while frames.next().await.is_some() {}
        })
        .await
        .unwrap();
        let expected = if missing {
            QuotaOutcome::Quit
        } else {
            QuotaOutcome::LogInAgain { target }
        };
        assert_eq!(*fixture.outcome.lock().unwrap(), Some(expected));
    }
}

#[tokio::test]
async fn remove_confirmation_defaults_to_cancel_for_control_enter_and_escape_without_an_effect() {
    let mut model = quota_two_account_view_model();
    model.rows[1].enabled = false;
    let target = model.rows[1].account_id.clone();
    let commands: QuotaAccountCommands = Arc::new(|command| {
        panic!("cancelling removal must not submit an account command: {command:?}")
    });
    let (queue, runner) = quota_account_command_runner(commands);
    let task = tokio::spawn(runner);
    let mut fixture = quota_control_fixture(QuotaFixtureOptions {
        view_model: model,
        ports: QuotaStatusViewOptions {
            focused_account_id: Some(target),
            account_commands: Some(queue.clone()),
            ..Default::default()
        },
        width: 120,
        height: 36,
        control: QuotaFixtureControl::Details,
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
        &mut fixture.element,
        MockTerminalConfig::with_events(events),
    );
    tokio::pin!(frames);
    account_checkpoint(frames.as_mut(), "Account details").await;
    for cancel in [
        control_event(KeyCode::Enter),
        provider_filter_key(KeyCode::Esc),
    ] {
        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
        let menu = account_checkpoint(frames.as_mut(), "Remove account…").await;
        let frame = menu.to_string();
        let (row, line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Remove account…"))
            .unwrap();
        let column = unicode_width::UnicodeWidthStr::width(
            line.get(..line.find("Remove account…").unwrap()).unwrap(),
        );
        sender
            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Down(MouseButton::Left),
                u16::try_from(column).unwrap(),
                u16::try_from(row).unwrap(),
            )))
            .unwrap();
        let confirmation = account_checkpoint(frames.as_mut(), "Confirm removal").await;
        assert!(
            confirmation.to_string().contains("beta")
                && confirmation.to_string().contains("cannot be undone")
        );
        // Lead taste review 01a18f46: the warning label is visibly separated.
        assert!(
            confirmation.to_string().contains("Warning:"),
            "{confirmation}"
        );
        // Lead bundle 01a18f9a / T7: spare rows follow the body, while the
        // standard one-row gap below the pinned header remains.
        let dialog = super::quota_ux_clarity_test::actual_dialog_text(
            &confirmation.to_string(),
            "Remove account",
        );
        let title_row = dialog
            .lines()
            .position(|line| line.contains("Remove account"))
            .unwrap();
        let content_row = dialog
            .lines()
            .position(|line| line.contains("Confirm removal"))
            .unwrap();
        assert_eq!(
            content_row,
            title_row + 2,
            "Remove body stays top-aligned: {dialog}"
        );
        sender.send(cancel).unwrap();
        let restored = account_checkpoint(frames.as_mut(), "Account details").await;
        assert!(!restored.to_string().contains("Confirm removal"));
    }
    sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
    sender
        .send(provider_filter_key(KeyCode::Char('q')))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while frames.next().await.is_some() {}
    })
    .await
    .unwrap();
    queue.close();
    assert!(
        task.await.unwrap().is_empty(),
        "neither cancellation submits an effect"
    );
    assert_eq!(*fixture.outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
}
