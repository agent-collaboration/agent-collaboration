//! Remove outcomes stay visible in their own dialog even when Browse is covered.
use crate::{
    AccountActionTarget, QuotaAccountAction, QuotaAccountCommandResult, QuotaAccountCommands,
    QuotaAccountRefusal, QuotaOutcome, QuotaStatusViewOptions, quota_account_command_runner,
    quota_browse_presentation_test::{
        quota_two_account_view_model, quota_ux_clarity_test::actual_dialog_text,
    },
    quota_fixture_view::{QuotaFixtureView, controlled_quota_fixture},
    test_support::{QuotaFixtureControl, QuotaFixtureOptions},
};
use futures_util::{Stream, StreamExt};
use iocraft::prelude::*;
use std::{pin::Pin, sync::Arc, time::Duration};

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

async fn painted<TFrames>(frames: Pin<&mut TFrames>, matches: impl FnMut(&Canvas) -> bool) -> Canvas
where
    TFrames: Stream<Item = Canvas> + ?Sized,
{
    crate::quota_frame_observation::next_matching_frame(frames, Duration::from_secs(2), matches)
        .await
        .unwrap()
}

fn wrapped_text(text: &str) -> String {
    text.lines()
        .map(|line| line.trim().trim_matches('│').trim())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

async fn remove_outcome_stays_in_dialog(result: QuotaAccountCommandResult, expected: &str) {
    for (width, height) in [(48, 12), (120, 36)] {
        // Arrange: keep the actual runner, observer, input owner and dialog renderer.
        let mut model = quota_two_account_view_model();
        model.rows[0].enabled = false;
        let target = AccountActionTarget::from_row(&model.rows[0], 0).unwrap();
        let expected_target = target.clone();
        let returned_result = result.clone();
        let commands: QuotaAccountCommands = Arc::new(move |command| {
            assert_eq!(command.target, expected_target);
            assert_eq!(command.action, QuotaAccountAction::Remove);
            let result = returned_result.clone();
            Box::pin(async move { result })
        });
        let (queue, runner) = quota_account_command_runner(commands);
        let mut completions = queue.subscribe();
        let running = tokio::spawn(runner);
        let view = QuotaFixtureView::default();
        let controller = view.controller.clone();
        let mut fixture = controlled_quota_fixture(
            QuotaFixtureOptions {
                view_model: model,
                ports: QuotaStatusViewOptions {
                    focused_account_id: Some(target.account_id.clone()),
                    account_commands: Some(queue.clone()),
                    ..Default::default()
                },
                width,
                height,
                control: QuotaFixtureControl::Details,
            },
            view,
        );
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
            &mut fixture.element,
            MockTerminalConfig::with_events(events),
        );
        tokio::pin!(frames);
        painted(frames.as_mut(), |canvas| {
            canvas.to_string().contains("Account details")
        })
        .await;
        sender.send(key(KeyCode::Enter)).unwrap();
        let menu = painted(frames.as_mut(), |canvas| {
            canvas.to_string().contains("Remove account…")
        })
        .await
        .to_string();
        let (row, line) = menu
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
        painted(frames.as_mut(), |canvas| {
            canvas.to_string().contains("Confirm removal")
        })
        .await;
        sender.send(key(KeyCode::Tab)).unwrap();
        painted(frames.as_mut(), |_| {
            controller
                .readback()
                .unwrap()
                .surfaces
                .top()
                .focus
                .focused()
                .is_some_and(|target| target.id.as_str() == "action:quota:confirm-remove")
        })
        .await;

        // Act: wait for the real completion and its published observer update, not its text.
        sender.send(key(KeyCode::Enter)).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    changed = completions.changed() => { changed.unwrap(); break; }
                    canvas = frames.next() => assert!(canvas.is_some(), "removal stays mounted"),
                }
            }
        })
        .await
        .unwrap();
        let completed = completions.borrow().clone().unwrap();
        let outcome = painted(frames.as_mut(), |canvas| {
            canvas.to_string().contains("Remove account")
                && controller.readback().unwrap().actions.iter().any(|action| {
                    action.id.as_str() == "quota:confirm-remove"
                        && action.availability.is_available()
                })
        })
        .await
        .to_string();
        let dialog = actual_dialog_text(&outcome, "Remove account");
        sender.send(key(KeyCode::Esc)).unwrap();
        painted(frames.as_mut(), |canvas| {
            canvas.to_string().contains("Account details")
        })
        .await;
        sender.send(key(KeyCode::Esc)).unwrap();
        let browse = painted(frames.as_mut(), |_| {
            controller.readback().unwrap().captured.is_none()
        })
        .await
        .to_string();
        sender.send(key(KeyCode::Char('q'))).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while frames.next().await.is_some() {}
        })
        .await
        .unwrap();
        queue.close();
        let receipts = running.await.unwrap();

        // Assert after teardown so the pre-fix content failure leaves no runner behind.
        assert_eq!(completed.command.target, target);
        assert_eq!(completed.result, result);
        assert_eq!(receipts, vec![completed]);
        assert_eq!(*fixture.outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
        assert!(
            wrapped_text(&dialog).contains(expected),
            "{width}×{height}: the named outcome must be inside Remove, not behind it:\n{outcome}"
        );
        assert!(
            wrapped_text(&browse).contains(expected),
            "the Browse notice is retained after closing Remove:\n{browse}"
        );
        if width == 120 {
            let title = outcome
                .lines()
                .position(|line| line.contains("Remove account"))
                .unwrap();
            let above_dialog = outcome.lines().take(title).collect::<Vec<_>>().join("\n");
            assert!(wrapped_text(&above_dialog).contains(expected), "{outcome}");
            assert!(dialog.contains("Confirm removal"), "{dialog}");
            assert!(
                wrapped_text(&dialog).contains("This cannot be undone."),
                "{dialog}"
            );
        }
    }
}

#[tokio::test]
async fn remove_outcome_refusal_is_visible_inside_the_short_confirmation_dialog() {
    remove_outcome_stays_in_dialog(
        QuotaAccountCommandResult::Refused {
            reason: QuotaAccountRefusal::RequiresDisabled,
        },
        "Disable the account before removing it",
    )
    .await;
}

#[tokio::test]
async fn remove_outcome_failure_is_visible_inside_the_short_confirmation_dialog() {
    let message =
        "sqlite state store failed: error returned from database: (code: 261) database is locked";
    remove_outcome_stays_in_dialog(
        QuotaAccountCommandResult::Failed {
            message: message.to_owned(),
        },
        message,
    )
    .await;
}
