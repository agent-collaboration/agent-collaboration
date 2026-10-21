use crate::presentation::quota::quota_browse_presentation_test::quota_two_account_view_model;
use crate::presentation::quota::quota_status_component::QuotaStatusComponent;
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn control(character: char) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}

#[tokio::test]
async fn quota_credit_choice_cancel_is_owned_before_editor_and_never_saves() {
    let saves = Arc::new(AtomicUsize::new(0));
    let saver: crate::presentation::quota::quota_account_options::CreditUsagePolicySaver = {
        let saves = saves.clone();
        Arc::new(move |_, _, policy| {
            saves.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(policy) })
        })
    };
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:120usize,height:36usize,credit_policy_saver:Some(saver)));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = frames.next().await {
            let text = canvas.to_string();
            match phase {
                0 => {
                    sender.send(control('r')).unwrap();
                    phase = 1;
                }
                1 if text.contains("Resets") => {
                    sender.send(key(KeyCode::Tab)).unwrap();
                    phase = 2;
                }
                2 if text.contains("Credit balance") => {
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 3;
                }
                3 if text.contains("Search:") => {
                    sender
                        .send(TerminalEvent::BracketedPaste("no-match-policy".to_owned()))
                        .unwrap();
                    phase = 4;
                }
                4 if text.contains("No matching choices") => {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 5;
                }
                5 if text.contains("Credit usage")
                    && text.contains("Save")
                    && !text.contains("Search:") =>
                {
                    assert_eq!(saves.load(Ordering::SeqCst), 0);
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 6;
                }
                6 if text.contains("Credit balance") && text.contains("Edit") => {
                    sender.send(control('d')).unwrap();
                    phase = 7;
                }
                _ => {}
            }
            last = text;
        }
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), journey).await;
    assert!(result.is_ok(), "phase={phase}: {last}");
    assert_eq!(phase, 7, "{last}");
    assert_eq!(saves.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn quota_wrapped_list_reveals_last_focused_account_before_exit() {
    let mut model = quota_two_account_view_model();
    let template = model.rows[0].clone();
    model.rows = (0..10)
        .map(|index| {
            let mut row = template.clone();
            row.account_id =
                codex_router_core::ids::AccountId::new(format!("account-{index}")).unwrap();
            row.account = format!("Account {index:02}");
            row.selected = index == 0;
            row
        })
        .collect();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component =
        element!(QuotaStatusComponent(view_model:model,width:76usize,height:24usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = frames.next().await {
            let text = canvas.to_string();
            match phase {
                0 => {
                    for _ in 0..9 {
                        sender.send(key(KeyCode::Down)).unwrap();
                    }
                    phase = 1;
                }
                1 if text.contains("❯ Account 09") => {
                    sender.send(control('d')).unwrap();
                    phase = 2;
                }
                _ => {}
            }
            last = text;
        }
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), journey).await;
    assert!(result.is_ok(), "phase={phase}: {last}");
    assert_eq!(phase, 2, "{last}");
}

#[tokio::test]
async fn quota_filter_no_match_stays_visible_above_actions_in_a_two_row_body() {
    for (width, height) in [(48usize, 12usize), (48, 24), (76, 30)] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(
            view_model:quota_two_account_view_model(),width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut phase = 0;
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let text = canvas.to_string();
                let visible_words = text
                    .chars()
                    .map(|character| {
                        if ('\u{2500}'..='\u{257f}').contains(&character) {
                            ' '
                        } else {
                            character
                        }
                    })
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                match phase {
                    0 => {
                        sender.send(control('f')).unwrap();
                        phase = 1;
                    }
                    1 if text.contains("Account filters") => {
                        sender.send(key(KeyCode::Enter)).unwrap();
                        sender
                            .send(TerminalEvent::BracketedPaste("zznomatchzz".to_owned()))
                            .unwrap();
                        phase = 2;
                    }
                    2 if visible_words.contains("No matching choices") => {
                        assert!(
                            text.contains("Account filters")
                                && text.contains("Cancel")
                                && text.contains("Select"),
                            "{text}"
                        );
                        // An empty Enter is deliberately a no-op and need not repaint.
                        // If it commits or closes the choice, this Escape dismisses
                        // the draft and the next required chooser frame is absent.
                        sender.send(key(KeyCode::Enter)).unwrap();
                        sender.send(key(KeyCode::Esc)).unwrap();
                        phase = 4;
                    }
                    4 if text.contains("Account filters") && !text.contains("No matching") => {
                        sender.send(key(KeyCode::Esc)).unwrap();
                        phase = 5;
                    }
                    5 if !text.contains("Account filters") => {
                        assert!(
                            text.contains("All") && text.contains("2/2"),
                            "draft cancellation preserves the committed filter: {text}"
                        );
                        sender.send(control('d')).unwrap();
                        phase = 6;
                    }
                    _ => {}
                }
                last = text;
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), journey).await;
        assert!(result.is_ok(), "{width}x{height} phase={phase}: {last}");
        assert_eq!(phase, 6, "{width}x{height}: {last}");
    }
}

#[tokio::test]
async fn quota_disabled_choices_remain_keyboard_reachable_in_a_two_row_body() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(
        view_model:quota_two_account_view_model(),width:48usize,height:12usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = frames.next().await {
            let text = canvas.to_string();
            match phase {
                0 => {
                    sender.send(control('f')).unwrap();
                    phase = 1;
                }
                1 if text.contains("Account filters") => {
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender.send(key(KeyCode::Down)).unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 2;
                }
                2 if text.contains("OpenAI ▾") && !text.contains("Search:") => {
                    eprintln!("PROVIDER_DRAFT_48x12_OPENAI\n{text}");
                    assert!(text.contains("Cancel") && text.contains("Apply"), "{text}");
                    sender.send(key(KeyCode::Down)).unwrap();
                    phase = 3;
                }
                3 if text.contains("Disabled accounts") && text.contains("Shown ▾") => {
                    eprintln!("PROVIDER_DRAFT_48x12_FIELDS\n{text}");
                    assert!(
                        text.contains("Disabled accounts") && text.contains("Shown ▾"),
                        "{text}"
                    );
                    assert!(text.contains("Cancel") && text.contains("Apply"), "{text}");
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 4;
                }
                4 if text.contains("❯ Shown") => {
                    eprintln!("DISABLED_CHOICE_48x12_SHOWN\n{text}");
                    assert!(text.contains("Cancel") && text.contains("Select"), "{text}");
                    sender.send(key(KeyCode::Down)).unwrap();
                    phase = 5;
                }
                5 if text.contains("❯ Hidden") => {
                    eprintln!("DISABLED_CHOICE_48x12_HIDDEN\n{text}");
                    assert!(text.contains("Cancel") && text.contains("Select"), "{text}");
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 6;
                }
                6 if text.contains("Account filters")
                    && text.contains("Hidden")
                    && !text.contains("Search:") =>
                {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 7;
                }
                7 if !text.contains("Account filters") => {
                    assert!(
                        text.contains("All") && text.contains("2/2"),
                        "discard must preserve the committed disabled filter: {text}"
                    );
                    sender.send(control('d')).unwrap();
                    phase = 8;
                }
                _ => {}
            }
            last = text;
        }
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), journey).await;
    assert!(result.is_ok(), "phase={phase}: {last}");
    assert_eq!(phase, 8, "{last}");
}
