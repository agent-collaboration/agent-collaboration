//! Shared mock-terminal fixtures; actual behavior cases live in focused modules.

use futures_util::{FutureExt, StreamExt};
use iocraft::MouseButton;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use super::*;
use crate::session_picker::provider_filter_model::SessionProviderFilter;
use crate::session_picker::test_support::observed_records;
use crate::session_picker::test_support::picker_record;
use crate::session_picker::test_support::picker_request;
use agent_session_inventory::picker_data_query::SessionsPickerDataQuery;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use agent_session_inventory::picker_data_query::SessionsPickerRoot as SessionsRoot;
use agent_session_inventory::picker_runtime_status::PickerRuntimeStatus;
use agent_session_inventory::sessions::SessionConversationPreview;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::SessionsProvider;
use agent_session_inventory::sessions::SessionsSort;
use agent_session_inventory::sessions::SessionsSource;

#[path = "picker_fork_tests.rs"]
mod picker_fork_tests;

#[path = "picker_performance_tests.rs"]
mod picker_performance_tests;

#[path = "picker_details_navigation_tests.rs"]
mod picker_details_navigation_tests;

#[path = "picker_scope_contract_tests.rs"]
mod picker_scope_contract_tests;

#[path = "picker_taste_review_tests.rs"]
mod picker_taste_review_tests;

#[path = "session_choice_ux_tests.rs"]
mod session_choice_ux_tests;

#[path = "native_catalogue_publication_tests.rs"]
mod native_catalogue_publication_tests;

#[path = "picker_grouping_component_tests.rs"]
mod picker_grouping_component_tests;

#[path = "picker_grouping_projection_tests.rs"]
mod picker_grouping_projection_tests;

#[path = "picker_grouping_persistence_tests.rs"]
mod picker_grouping_persistence_tests;

#[path = "picker_menu_section_tests.rs"]
mod picker_menu_section_tests;

// Blocking external fixture procedures run off the executor; production loaders are async.
fn fixture_record_loader<FixtureProcedure>(
    procedure: FixtureProcedure,
) -> SessionsPickerRecordLoader
where
    FixtureProcedure: Fn(
            SessionsPickerDataQuery,
        )
            -> Result<agent_session_inventory::picker_runtime_status::PickerRecordsSnapshot, String>
        + Send
        + Sync
        + 'static,
{
    let procedure = Arc::new(procedure);
    Arc::new(move |request| {
        let procedure = Arc::clone(&procedure);
        Box::pin(async move {
            let query = request.query.clone();
            match tokio::task::spawn_blocking(move || procedure(query)).await {
                Ok(Ok(snapshot)) => agent_session_inventory::source_inventory_request::SourceInventoryResult::Ready { progress: agent_session_inventory::source_inventory_request::InventoryPageProgress::Complete, binding: None, request, snapshot },
                Ok(Err(_)) | Err(_) => agent_session_inventory::source_inventory_request::SourceInventoryResult::Rejected {
                    request, reason: agent_session_inventory::source_inventory_request::SourceInventoryRejection::SourceUnavailable,
                },
            }
        })
    })
}

fn reload_query(search: &str) -> SessionsPickerDataQuery {
    SessionsPickerDataQuery {
        root: SessionsRoot::Any,
        provider: SessionsProvider::Any,
        source: SessionsSource::All,
        sort: SessionsSort::Updated,
        search: search.to_owned(),
        include_empty_sessions: false,
    }
}

async fn render_picker_capture(
    request: SessionsPickerRequest,
    width: usize,
    events: Vec<TerminalEvent>,
) -> String {
    render_picker_capture_at(request, width, MIN_RENDER_HEIGHT, events).await
}

pub(super) async fn render_picker_capture_at(
    request: SessionsPickerRequest,
    width: usize,
    height: usize,
    events: Vec<TerminalEvent>,
) -> String {
    render_counted_picker_capture_at(request, width, height, events)
        .await
        .text
}

#[derive(Clone, Default)]
struct PickerCapture {
    text: String,
    visible_records: usize,
    total_records: usize,
    machine_label: String,
    committed_provider_filter: Option<SessionProviderFilter>,
}

async fn render_counted_picker_capture_at(
    request: SessionsPickerRequest,
    width: usize,
    height: usize,
    mut events: Vec<TerminalEvent>,
) -> PickerCapture {
    // R4: Escape is no longer a test termination path.
    events.push(ctrl_key('d'));
    let observed = Arc::new(Mutex::new(PickerCapture::default()));
    let sink = observed.clone();
    let frames = element! {
        SessionsPickerComponent(
            request,
            width,
            height,
            surface_observer: move |(_,model): (agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle,State<SessionsPickerModel>)| {
                let model=model.read();
                let mut snapshot=sink.lock().unwrap();
                snapshot.visible_records=model.visible_record_len();
                snapshot.total_records=model.request.records.len();
                snapshot.machine_label=model.machine_controls.label(model.request.machine_mode).to_owned();
                snapshot.committed_provider_filter=Some(model.provider_filter);
            },
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        events,
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    let mut capture = observed.lock().unwrap().clone();
    capture.text = frames
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("picker should render at least one frame"));
    capture
}

fn meaningful_line_count(text: &str) -> usize {
    text.lines().count()
}

fn visible_followup_row_count(text: &str) -> usize {
    text.lines()
        .filter(|line| line.contains("Follow-up implementation lane"))
        .count()
}

fn has_sidecar_details(text: &str) -> bool {
    text.lines()
        .any(|line| line.matches('┌').count() >= 2 && line.matches('┐').count() >= 2)
}

pub(super) fn ctrl_key(character: char) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}

pub(super) fn control_shift_enter_key() -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    event.modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    TerminalEvent::Key(event)
}

fn capture_picker_request() -> SessionsPickerRequest {
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.source = SessionsSource::All;
    for index in 0..8 {
        request.records.push(capture_record(
            &format!("thread-extra-{index}"),
            &format!("Follow-up implementation lane {index}"),
            "/repo/project-a",
            "codex-router",
            "cli",
        ));
    }
    request
}

fn capture_record(
    session_id: &str,
    title: &str,
    cwd: &str,
    provider: &str,
    source: &str,
) -> SessionPickerRecord {
    SessionPickerRecord {
        identity: agent_session_inventory::sessions::SessionPickerIdentity::LocalCodex(
            session_id.to_owned(),
        ),
        provider_family:
            agent_session_inventory::session_provider_family::SessionProviderFamily::Codex,
        source_context: None,
        machine_display_label: None,
        provenance: agent_session_inventory::sessions::SessionRowProvenance::LocalHomeCatalog,
        endpoint_label: None,
        provider_state: None,
        native_background: None,
        native_peer: None,
        native_conflict: None,
        native_inventory_view: None,
        session_id: session_id.to_owned(),
        title: title.to_owned(),
        full_title: title.to_owned(),
        explicit_name: None,
        recency: "now".to_owned(),
        created: "1d ago".to_owned(),
        recency_at_ms: Some(2_000),
        created_at_ms: Some(1_000),
        branch: "main".to_owned(),
        persisted_branch: "main".to_owned(),
        context: cwd.rsplit('/').next().unwrap_or(cwd).to_owned(),
        cwd: Some(cwd.to_owned()),
        normalized_cwd: Some(cwd.to_owned()),
        git_origin_url: Some("https://github.com/shravan-agent/codex-router.git".to_owned()),
        provider: Some(provider.to_owned()),
        model: Some("gpt-5-codex".to_owned()),
        reasoning_effort: Some("low".to_owned()),
        preview: Some(format!("{title} preview text")),
        first_user_message: format!("{title} recent question"),
        conversation: SessionConversationPreview {
            snippets: vec![
                format!("{title} recent question"),
                format!("{title} recent answer"),
            ],
            unavailable_reason: None,
        },
        conversation_source: None,
        source: Some(source.to_owned()),
        thread_source: Some(source.to_owned()),
        native_source: None,
        runtime_status: PickerRuntimeStatus::Unknown,
    }
}

fn capture_dir() -> PathBuf {
    let dir = std::env::var_os("CODEX_ROUTER_CAPTURE_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/ux-proof/production"),
        PathBuf::from,
    );
    must_ok(std::fs::create_dir_all(&dir));
    dir
}

fn write_capture_pair(dir: &Path, name: &str, text: &str) {
    must_ok(std::fs::write(dir.join(format!("{name}.txt")), text));
    must_ok(std::fs::write(
        dir.join(format!("{name}.svg")),
        terminal_svg(name, text),
    ));
}

fn terminal_svg(title: &str, text: &str) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(1);
    let height = lines.len().max(1);
    let pixel_width = width * 9 + 32;
    let pixel_height = height * 18 + 34;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{pixel_width}\" height=\"{pixel_height}\" viewBox=\"0 0 {pixel_width} {pixel_height}\"><rect width=\"100%\" height=\"100%\" fill=\"#111318\"/>"
    );
    svg.push_str(&format!(
            "<text x=\"16\" y=\"24\" xml:space=\"preserve\" font-family=\"SFMono-Regular, Menlo, Consolas, monospace\" font-size=\"14\" fill=\"#e6edf3\"><tspan>{}</tspan>",
            escape_xml(title)
        ));
    for (index, line) in lines.iter().enumerate() {
        svg.push_str(&format!(
            "<tspan x=\"16\" dy=\"{}\">{}</tspan>",
            if index == 0 { 20 } else { 18 },
            escape_xml(line)
        ));
    }
    svg.push_str("</text></svg>");
    svg
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn must_ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("expected Ok, got error: {error}"),
    }
}

#[path = "picker_interaction_tests.rs"]
mod picker_interaction_tests;

#[path = "provider_filter_component_tests.rs"]
mod provider_filter_component_tests;

#[path = "provider_filter_focus_reveal_tests.rs"]
mod provider_filter_focus_reveal_tests;

#[path = "picker_layout_tests.rs"]
mod picker_layout_tests;

#[path = "picker_pane_stability_tests.rs"]
mod picker_pane_stability_tests;

#[path = "picker_background_tests.rs"]
mod picker_background_tests;

#[path = "picker_capture_tests.rs"]
mod picker_capture_tests;

#[path = "picker_machine_control_tests.rs"]
mod picker_machine_control_tests;

#[path = "picker_single_view_tests.rs"]
mod picker_single_view_tests;

#[path = "picker_compact_header_tests.rs"]
mod picker_compact_header_tests;

#[path = "picker_help_tests.rs"]
mod picker_help_tests;

#[path = "source_action_component_tests.rs"]
mod source_action_component_tests;

fn resume_key() -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}

#[path = "picker_pointer_tests.rs"]
mod picker_pointer_tests;

async fn next_current_canvas<TCanvases>(
    mut canvases: std::pin::Pin<&mut TCanvases>,
) -> std::io::Result<Option<iocraft::Canvas>>
where
    TCanvases: futures_util::Stream<Item = iocraft::Canvas> + ?Sized,
{
    let Some(mut current) = canvases.next().await else {
        return Ok(None);
    };
    for _ in 0..64 {
        match canvases.next().now_or_never() {
            Some(Some(canvas)) => current = canvas,
            _ => return Ok(Some(current)),
        }
    }
    Err(std::io::Error::other(format!(
        "ready-frame drain exceeded64: {current}"
    )))
}

#[path = "session_context_ux_tests.rs"]
mod session_context_ux_tests;

#[derive(Clone, Copy)]
struct DialogBounds {
    top: usize,
    left: usize,
    right: usize,
    bottom: usize,
}

async fn render_anchored_picker_canvases(
    request: SessionsPickerRequest,
    width: usize,
    height: usize,
    events_before_quit: Vec<TerminalEvent>,
    outcome: Option<&mut Option<SessionsPickerOutcome>>,
    expected_kind: agent_collaboration_tui::surfaces::surface_stack::OverlayKind,
) -> (Vec<iocraft::Canvas>, DialogBounds) {
    use agent_collaboration_tui::surfaces::{
        hit_regions::HitRegionKind,
        surface_handle::SurfaceStackHandle,
        surface_stack::{OverlayLayout, SurfaceId},
    };

    let expected_inputs = events_before_quit.len();
    let processed = Arc::new(AtomicUsize::new(0));
    let processed_sink = processed.clone();
    let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
    let sink = observed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    for event in events_before_quit {
        sender.send(event).unwrap();
    }
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(SessionsPickerComponent(request,width,height,
        selected_outcome_out:outcome,
        surface_observer:move |(stack,_)|*sink.lock().unwrap()=Some(stack),
        input_observer:move |()| {processed_sink.fetch_add(1,Ordering::SeqCst);}));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut captured = Vec::new();
    let mut popup_bounds = None;
    let journey = async {
        while let Some(canvas) = frames.next().await {
            if popup_bounds.is_none() && processed.load(Ordering::SeqCst) >= expected_inputs {
                let stack = observed.lock().unwrap().as_ref().copied().unwrap();
                let snapshot = stack.snapshot();
                if let Some(overlay) = snapshot.overlays().last()
                    && overlay.kind == expected_kind
                    && let OverlayLayout::Anchored { target, .. } = &overlay.layout
                    && let Some(map) = stack.hit_regions().published()
                    && map.targets.iter().any(|region| {
                        region.surface == overlay.parent
                            && region.target == *target
                            && region.kind == HitRegionKind::Target
                            && !region.cells.is_empty()
                    })
                    && let Some(region) = map.surfaces.iter().find(|region| {
                        region.surface == SurfaceId::Overlay(overlay.id) && !region.cells.is_empty()
                    })
                {
                    // Both geometries come from the published draw after the open
                    // acknowledgement. No frame count or timing wait stands in for it.
                    popup_bounds = Some(DialogBounds {
                        top: region
                            .cells
                            .iter()
                            .map(|(_, row)| usize::from(*row))
                            .min()
                            .unwrap(),
                        bottom: region
                            .cells
                            .iter()
                            .map(|(_, row)| usize::from(*row))
                            .max()
                            .unwrap(),
                        left: region
                            .cells
                            .iter()
                            .map(|(column, _)| usize::from(*column))
                            .min()
                            .unwrap(),
                        right: region
                            .cells
                            .iter()
                            .map(|(column, _)| usize::from(*column))
                            .max()
                            .unwrap(),
                    });
                    sender.send(ctrl_key('d')).unwrap();
                }
            }
            captured.push(canvas);
        }
    };
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("the open input must publish its anchor and active popup before quitting");
    (captured, popup_bounds.expect("published anchored popup"))
}

fn dialog_bounds(frame: &str, title: &str) -> DialogBounds {
    let lines = frame.lines().collect::<Vec<_>>();
    let title_row = lines
        .iter()
        .position(|line| line.contains(title))
        .expect("actual dialog title");
    let top = (0..title_row)
        .rev()
        .find(|row| lines[*row].contains('╭') && lines[*row].contains('╮'))
        .expect("actual modal top border");
    let left = unicode_width::UnicodeWidthStr::width(
        lines[top].get(..lines[top].find('╭').unwrap()).unwrap(),
    );
    let right = unicode_width::UnicodeWidthStr::width(
        lines[top].get(..lines[top].find('╮').unwrap()).unwrap(),
    );
    let bottom = (title_row..lines.len())
        .find(|row| {
            lines[*row].match_indices('╰').any(|(byte, _)| {
                unicode_width::UnicodeWidthStr::width(lines[*row].get(..byte).unwrap()) == left
            })
        })
        .unwrap_or_else(|| {
            panic!("matching modal bottom border for {title}, top {top}, left {left}: {frame}")
        });
    DialogBounds {
        top,
        left,
        right,
        bottom,
    }
}

fn dialog_text(frame: &str, bounds: DialogBounds) -> String {
    frame
        .lines()
        .skip(bounds.top + 1)
        .take(bounds.bottom - bounds.top - 1)
        .map(|line| {
            line.chars()
                .scan(0usize, |column, character| {
                    let start = *column;
                    *column += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
                    Some((start, character))
                })
                .filter(|(column, _)| *column > bounds.left && *column < bounds.right)
                .map(|(_, character)| if character == '┃' { ' ' } else { character })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dropdown_reason_text(dropdown: &str, prefix: &str) -> String {
    let lines = dropdown.lines().collect::<Vec<_>>();
    let (row, line) = lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains(prefix))
        .expect("visible refusal prefix");
    let column = line
        .get(..line.find(prefix).unwrap())
        .unwrap()
        .chars()
        .count();
    lines[row..]
        .iter()
        .map(|line| {
            let count = line.chars().count();
            line.chars()
                .skip(column)
                .take(count.saturating_sub(column + 1))
                .collect::<String>()
        })
        .take_while(|line| !line.trim().is_empty())
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
