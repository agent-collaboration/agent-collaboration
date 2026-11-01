//! Focused fixture proof keeps input, controls, source publication and outcomes real.
use crate::session_picker::PickerSourceContext;
use crate::session_picker::SessionsPickerOutcome;
use crate::session_picker::screen::fixtures::picker_fixture_receipts::write_outcome;
use crate::session_picker::screen::fixtures::picker_fixture_scenarios;
use crate::session_picker::screen::fixtures::picker_fixture_scenarios::PickerFixtureScenario;
use crate::session_picker::screen::fixtures::provider_filter_storyboard_fixture::apply_storyboard_variant;
use crate::session_picker::screen::fixtures::provider_filter_storyboard_fixture::provider_filter_storyboard_request;
use crate::session_picker::screen::picker_component::SessionsPickerComponent;
use crate::session_picker::screen::picker_model::SessionsPickerModel;
use agent_collaboration_tui::foundation::theme::SurfaceRole;
use agent_collaboration_tui::foundation::theme::Theme;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use agent_collaboration_tui::surfaces::surface_stack::OverlayKind;
use agent_session_inventory::sessions::source_client_action::SessionClientEffect;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeClaudeAction;
use futures_util::StreamExt;
use iocraft::Canvas;
use iocraft::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn ctrl(code: KeyCode) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, code);
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}
#[derive(Default)]
struct FixtureStep {
    events: Vec<TerminalEvent>,
    release_source: bool,
}
impl From<Vec<TerminalEvent>> for FixtureStep {
    fn from(events: Vec<TerminalEvent>) -> Self {
        Self {
            events,
            release_source: false,
        }
    }
}

async fn exercise(
    name: &str,
    directory: &Path,
    initial: Vec<TerminalEvent>,
    next: impl FnMut(&Canvas, SurfaceStackHandle, State<SessionsPickerModel>) -> FixtureStep,
) -> Option<SessionsPickerOutcome> {
    exercise_at(name, directory, initial, next, 76, 30).await
}

async fn exercise_at(
    name: &str,
    directory: &Path,
    initial: Vec<TerminalEvent>,
    mut next: impl FnMut(&Canvas, SurfaceStackHandle, State<SessionsPickerModel>) -> FixtureStep,
    width: usize,
    height: usize,
) -> Option<SessionsPickerOutcome> {
    let request =
        apply_storyboard_variant(provider_filter_storyboard_request().unwrap(), Some(name))
            .unwrap();
    let captured_request = request.clone();
    let scenario = PickerFixtureScenario::from_name(name);
    let ports = picker_fixture_scenarios::client_ports(scenario, &request);
    let grouping_clock = picker_fixture_scenarios::grouping_clock(scenario).unwrap();
    let source_preparer = picker_fixture_scenarios::source_preparer(scenario, &request)
        .unwrap_or_else(|| {
            crate::session_picker::screen::fixtures::picker_fixture_receipts::recording_preparer(
                Some(directory),
            )
        });
    let loader = picker_fixture_scenarios::picker_fixture_source_updates::record_loader(
        scenario,
        Some(directory),
    )
    .unwrap();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut submitted_inputs = initial.len();
    for event in initial {
        sender.send(event).unwrap();
    }
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let observed = Arc::new(Mutex::new(None));
    let observer = observed.clone();
    let processed_inputs = Arc::new(AtomicUsize::new(0));
    let input_observer = processed_inputs.clone();
    let mut outcome = None;
    {
        let mut component = element!(SessionsPickerComponent(
            request, record_loader:loader,
            grouping_clock,
            source_preparer:Some(source_preparer),
            details_client_inspector:ports.as_ref().map(|ports|ports.details_inspector.clone()),
            client_inventory_loader:ports.as_ref().map(|ports|ports.inventory_loader.clone()),
            client_executor:ports.as_ref().map(|ports|ports.executor.clone()),
            width,height,selected_outcome_out:&mut outcome,
            surface_observer:move |value|{*observer.lock().unwrap()=Some(value)},
            input_observer:move |_| { input_observer.fetch_add(1,Ordering::Release); }
        ));
        let mut frames =
            Box::pin(component.mock_terminal_render_loop(MockTerminalConfig::with_events(events)));
        let mut releases = tokio::task::JoinSet::new();
        loop {
            let mut last = String::new();
            let step = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while let Some(canvas) = frames.next().await {
                    last=canvas.to_string();
                    // A layout/source frame does not establish input consumption.
                    if processed_inputs.load(Ordering::Acquire) < submitted_inputs {
                        continue;
                    }
                    let (stack, model) = observed.lock().unwrap().unwrap();
                    let step = next(&canvas, stack, model);
                    if step.release_source || !step.events.is_empty() {
                        return Some(step);
                    }
                }
                None
            })
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "{name}: waiting for published fixture state after {submitted_inputs} inputs: {last}"
                )
            });
            let Some(step) = step else {
                break;
            };
            if step.release_source {
                let socket = directory.join("source-update.sock");
                releases.spawn(async move {
                    let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
                    stream.write_all(b"update\n").await.unwrap();
                    stream.shutdown().await.unwrap();
                    let mut response = Vec::new();
                    stream.read_to_end(&mut response).await.unwrap();
                    assert_eq!(response, b"prepared\n");
                });
            }
            for event in step.events {
                submitted_inputs += 1;
                sender.send(event).unwrap();
            }
        }
        while let Some(released) =
            tokio::time::timeout(std::time::Duration::from_secs(2), releases.join_next())
                .await
                .expect("fixture source-update task settles its real ACK")
        {
            released.unwrap();
        }
    }
    write_outcome(directory, &outcome, &captured_request).unwrap();
    outcome
}

fn highlighted_option(canvas: &Canvas, stack: SurfaceStackHandle) -> Option<String> {
    let map = stack.hit_regions().published()?;
    map.targets
        .iter()
        .find(|region| {
            region.surface == stack.snapshot().top_id()
                && region.target.as_str().contains("option:")
                && region.cells.iter().any(|&(column, row)| {
                    canvas
                        .cell(usize::from(column), usize::from(row))
                        .is_some_and(|cell| {
                            cell.background_color
                                == Some(Theme::default().background(SurfaceRole::Selected))
                        })
                })
        })
        .map(|region| region.target.as_str().to_owned())
}

fn dropdown_open(stack: SurfaceStackHandle) -> bool {
    stack
        .snapshot()
        .overlays()
        .last()
        .is_some_and(|overlay| matches!(overlay.kind, OverlayKind::Dropdown { .. }))
}

#[tokio::test]
async fn stopped_native_claude_fixture_reaches_open_and_records_the_captured_resume() {
    let directory = tempfile::tempdir().unwrap();
    let mut phase = 0;
    let outcome = exercise(
        "stopped-native-claude",
        directory.path(),
        vec![
            key(KeyCode::Char('/')),
            TerminalEvent::BracketedPaste("Stopped native Claude".into()),
            key(KeyCode::F(4)),
        ],
        |canvas, stack, model| {
            let text = canvas.to_string();
            if phase == 0 && text.contains("Session details") && text.contains("Open") {
                let dialog = model.read().details_dialog.clone().unwrap();
                assert_eq!(
                    dialog.record().native_background.as_ref().unwrap().state,
                    collaboration_client::protocol::NativeClaudeBackgroundState::Stopped
                );
                assert!(!text.contains("Resume in background"));
                if stack
                    .snapshot()
                    .top()
                    .focus
                    .current()
                    .is_some_and(|target| target.as_str() == "action:details-open")
                {
                    phase = 1;
                    return vec![key(KeyCode::Enter)].into();
                }
                return vec![key(KeyCode::Tab)].into();
            }
            FixtureStep::default()
        },
    )
    .await;
    assert_eq!(phase, 1);
    let Some(SessionsPickerOutcome::LaunchNativeClaude(captured, _)) = outcome else {
        panic!("expected captured native terminal handoff");
    };
    assert!(
        matches!(&captured.effect, SessionClientEffect::Native {action:SourceNativeClaudeAction::Resume {directory,arguments,..},..} if directory.as_str()=="/fixture/stopped-claude"&&arguments.is_empty())
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("picker-outcome.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt,
        serde_json::json!({"kind":"resume","identity":{"kind":"nativeClaude","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000011"}}})
    );
}

#[tokio::test]
async fn local_codex_new_fixture_reaches_local_machine_and_returns_local_new() {
    let directory = tempfile::tempdir().unwrap();
    let mut reached = false;
    let outcome = exercise(
        "local-codex-new",
        directory.path(),
        vec![ctrl(KeyCode::Char('n'))],
        |canvas, _, model| {
            if !reached && canvas.to_string().contains("New session") {
                assert_eq!(
                    model.read().new_session.as_ref().unwrap().selected_source,
                    PickerSourceContext::LocalCodex
                );
                assert!(canvas.to_string().contains("Local Codex"));
                reached = true;
                return vec![ctrl(KeyCode::Enter)].into();
            }
            FixtureStep::default()
        },
    )
    .await;
    assert!(reached);
    assert_eq!(outcome, Some(SessionsPickerOutcome::StartNewSession));
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("picker-outcome.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt,
        serde_json::json!({"kind":"new","request":{"client":"codex","machine":"localCodex","directory":"/repo/project-a","runMode":"foreground"}})
    );
}

#[tokio::test]
async fn uncommitted_machine_fixture_has_a_distinct_highlight_without_retargeting_start() {
    let directory = tempfile::tempdir().unwrap();
    let mut phase = 0;
    let outcome = exercise(
        "new-machine-uncommitted",
        directory.path(),
        vec![ctrl(KeyCode::Char('n'))],
        |canvas, stack, model| {
            if phase == 0 && canvas.to_string().contains("New session") {
                phase = 1;
                return vec![key(KeyCode::Tab), key(KeyCode::Enter), key(KeyCode::Down)].into();
            }
            if phase == 1
                && dropdown_open(stack)
                && highlighted_option(canvas, stack).is_some_and(|target| {
                    target.ends_with("option:machine:configured:Fixture remote")
                })
            {
                assert_eq!(
                    model.read().new_session.as_ref().unwrap().selected_source,
                    PickerSourceContext::DefaultHosted
                );
                phase = 2;
                return vec![ctrl(KeyCode::Enter)].into();
            }
            FixtureStep::default()
        },
    )
    .await;
    assert_eq!(
        phase, 2,
        "the distinct remote highlight must actually be drawn"
    );
    assert_eq!(outcome, Some(SessionsPickerOutcome::StartNewSession));
    assert!(!directory.path().join("source-preparation.json").exists());
}

async fn source_options_update(name: &str, expected: &[&str], removed: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut phase = 0;
    let outcome = exercise(
        name,
        directory.path(),
        vec![
            ctrl(KeyCode::Char('f')),
            key(KeyCode::Tab),
            key(KeyCode::Tab),
            key(KeyCode::Enter),
        ],
        |canvas, stack, model| {
            if phase == 0 && dropdown_open(stack) {
                phase = 1;
                let mut events = vec![TerminalEvent::BracketedPaste("provider".into())];
                events.extend(vec![
                    key(KeyCode::Down);
                    if name == "source-options-add" { 2 } else { 3 }
                ]);
                return events.into();
            }
            if phase == 1
                && highlighted_option(canvas, stack).is_some_and(|target| {
                    target.ends_with("option:filter:ModelProvider(Id(\"beta-provider\"))")
                })
            {
                phase = 2;
                return FixtureStep {
                    events: Vec::new(),
                    release_source: true,
                };
            }
            if phase == 2 && dropdown_open(stack) {
                let providers = model
                    .read()
                    .request
                    .records
                    .iter()
                    .filter_map(|row| row.provider.clone())
                    .collect::<std::collections::BTreeSet<_>>();
                if providers
                    != expected
                        .iter()
                        .map(|provider| (*provider).to_owned())
                        .collect()
                {
                    return FixtureStep::default();
                }
                let highlight = highlighted_option(canvas, stack);
                if removed {
                    assert_eq!(highlight, None);
                } else {
                    assert!(highlight.is_some_and(|target| {
                        target.ends_with("option:filter:ModelProvider(Id(\"beta-provider\"))")
                    }));
                }
                assert_eq!(
                    model
                        .read()
                        .provider_filter_draft
                        .as_ref()
                        .unwrap()
                        .query
                        .provider,
                    agent_session_inventory::sessions::SessionsProvider::Any
                );
                phase = 3;
                let mut events = vec![key(KeyCode::Enter)];
                if removed {
                    events.push(key(KeyCode::Backspace));
                }
                return events.into();
            }
            if phase == 3 {
                let provider = model
                    .read()
                    .provider_filter_draft
                    .as_ref()
                    .unwrap()
                    .query
                    .provider
                    .clone();
                if removed {
                    assert!(dropdown_open(stack));
                    assert_eq!(highlighted_option(canvas, stack), None);
                    assert_eq!(
                        provider,
                        agent_session_inventory::sessions::SessionsProvider::Any
                    );
                } else {
                    assert!(!dropdown_open(stack));
                    assert_eq!(
                        provider,
                        agent_session_inventory::sessions::SessionsProvider::Id(
                            "beta-provider".to_owned()
                        )
                    );
                }
                phase = 4;
                return vec![ctrl(KeyCode::Char('d'))].into();
            }
            FixtureStep::default()
        },
    )
    .await;
    assert_eq!(
        phase, 4,
        "{name}: source update must publish while dropdown is open"
    );
    assert_eq!(outcome, None);
    assert!(
        !directory.path().join("source-update.sock").exists(),
        "source port lifetime ends with the picker"
    );
}

#[tokio::test]
async fn source_options_add_fixture_keeps_the_highlight_after_its_index_shifts() {
    source_options_update(
        "source-options-add",
        &["alpha-provider", "beta-provider", "omega-provider"],
        false,
    )
    .await;
}

#[tokio::test]
async fn source_options_remove_fixture_does_not_replace_or_commit_the_removed_highlight() {
    source_options_update(
        "source-options-remove",
        &["alpha-provider", "omega-provider"],
        true,
    )
    .await;
}

#[tokio::test]
async fn source_options_replace_fixture_keeps_a_surviving_key_and_commits_it() {
    source_options_update(
        "source-options-replace",
        &["beta-provider", "zeta-provider"],
        false,
    )
    .await;
}

#[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
#[cfg(test)]
pub(crate) mod picker_fixture_additional_scenario_test;

#[cfg(all(feature = "quota-reset-test-harness", debug_assertions))]
#[cfg(test)]
pub(crate) mod picker_fixture_grouping_test;
