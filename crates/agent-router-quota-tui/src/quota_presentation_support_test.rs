async fn quota_menu_canvas(view_model:QuotaStatusViewModel,width:usize,height:usize)->iocraft::Canvas {
    let (sender,receiver)=tokio::sync::mpsc::unbounded_channel();
    let events=futures_util::stream::unfold(receiver,|mut receiver|async {receiver.recv().await.map(|event|(event,receiver))});
    let outcome=Arc::new(std::sync::Mutex::new(None));
    let mut component=element!(QuotaStatusComponent(view_model,width,height,spinner_interval:Duration::from_secs(3600),outcome_sink:Some(outcome.clone())));
    let frames=crate::quota_test_support::recording_backend::recording_terminal_render_loop(&mut component,MockTerminalConfig::with_events(events));tokio::pin!(frames);
    let canvas=tokio::time::timeout(Duration::from_secs(2),async {
        crate::quota_frame_observation::next_current_frame(frames.as_mut()).await.unwrap().unwrap();
        sender.send(provider_filter_key(KeyCode::F(10))).unwrap();
        let mut canvas=crate::quota_frame_observation::next_matching_frame(frames.as_mut(),Duration::from_secs(2),|canvas|{let frame=canvas.to_string();frame.contains("❯ Details")||frame.contains("❯ Filters")}).await.unwrap();
        for _ in 0..16 {let frame=canvas.to_string();if frame.contains("Help")&&frame.contains("Quit") {break;}sender.send(provider_filter_key(KeyCode::Down)).unwrap();canvas=crate::quota_frame_observation::next_current_frame(frames.as_mut()).await.unwrap().unwrap();}
        assert!(canvas.to_string().contains("Help")&&canvas.to_string().contains("Quit"),"complete menu entries: {canvas}");
        let mut quit=KeyEvent::new(KeyEventKind::Press,KeyCode::Char('c'));quit.modifiers=KeyModifiers::CONTROL;sender.send(TerminalEvent::Key(quit)).unwrap();
        while frames.next().await.is_some() {}
        canvas
    }).await.expect("F10 menu must render before the actual quota quit boundary");
    assert_eq!(*outcome.lock().unwrap(),Some(crate::QuotaOutcome::Quit));canvas
}

pub(crate) fn quota_help_title_row(frame:&str)->Option<usize> {
    frame.lines().position(|line|line.split('│').any(|segment| {let text=segment.trim();text=="Help" || (text.starts_with("Help ")&&!text.starts_with("Help F1"))}))
}
pub(crate) fn quota_help_visible(frame:&str)->bool {quota_help_title_row(frame).is_some()}

async fn render_quota_capture(width: usize) -> String {
    render_quota_capture_model_at(
        quota_view_model(),
        width,
        MIN_RENDER_HEIGHT,
        vec![TerminalEvent::Key(KeyEvent::new(
            KeyEventKind::Press,
            KeyCode::Char('q'),
        ))],
    )
    .await
}

async fn render_quota_capture_model_at(
    view_model: QuotaStatusViewModel,
    width: usize,
    height: usize,
    events: Vec<TerminalEvent>,
) -> String {
    let frames = render_quota_capture_frames(view_model, width, height, events).await;
    frames
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("quota status should render at least one frame"))
}

#[derive(Props)]
struct QuotaCaptureHarnessProps {
    view_model:QuotaStatusViewModel,width:usize,height:usize,
    #[iocraft(required)] journal:crate::quota_test_support::event_driver::EventJournal,
    #[iocraft(required)] outcome:Arc<std::sync::Mutex<Option<crate::QuotaOutcome>>>,
}
#[component]
fn QuotaCaptureHarness(props:&QuotaCaptureHarnessProps,mut hooks:Hooks)->impl Into<AnyElement<'static>> {
    crate::quota_test_support::event_driver::record_draw(&mut hooks,&props.journal);
    let journal=props.journal.clone();
    element!(QuotaStatusComponent(view_model:props.view_model.clone(),width:props.width,height:props.height,spinner_interval:Duration::from_secs(3600),outcome_sink:Some(props.outcome.clone()),routed_outcome_observer:Some(Handler::from(move |outcome|journal.processed(crate::quota_test_support::event_driver::SurfaceSnapshot(vec![(agent_collaboration_tui::surface_stack::SurfaceId::Screen,format!("{outcome:?}").into_bytes())]))))))
}

async fn render_quota_capture_frames(
    view_model:QuotaStatusViewModel,width:usize,height:usize,mut events:Vec<TerminalEvent>,
)->Vec<String> {
    use crate::quota_test_support::{event_driver::{EventDriver,InputMode},recording_backend::recording_terminal_render_loop,frame_grid::FrameGrid};
    // Only the final event is a capture terminator. Intermediate Esc keeps its
    // close/cancel role. The retired Esc-exit terminator becomes actual Ctrl+C.
    let mut quit=KeyEvent::new(KeyEventKind::Press,KeyCode::Char('c'));quit.modifiers=KeyModifiers::CONTROL;
    let mut terminator=TerminalEvent::Key(quit);
    if let Some(TerminalEvent::Key(key))=events.last() {
        let final_quit=key.code==KeyCode::Esc||key.code==KeyCode::Char('q')||super::quota_reset_component_input::forced_quota_exit_key(key.code,key.modifiers);
        if final_quit {
            let event=events.pop().unwrap();
            if !matches!(&event,TerminalEvent::Key(key) if key.code==KeyCode::Esc) {terminator=event;}
        }
    }
    let (mut driver,mut config,journal)=EventDriver::new();
    let (finish,receiver)=tokio::sync::mpsc::unbounded_channel();
    let ending=futures_util::stream::unfold(receiver,|mut receiver|async {receiver.recv().await.map(|event|(event,receiver))});
    config.events=futures_util::stream::select(config.events,ending).boxed();
    let outcome=Arc::new(std::sync::Mutex::new(None));
    let mut element=element!(QuotaCaptureHarness(view_model,width,height,journal,outcome:outcome.clone()));
    let captures=recording_terminal_render_loop(&mut element,config);tokio::pin!(captures);
    tokio::time::timeout(Duration::from_secs(10),async {
        driver.initial(&mut captures).await;
        driver.drive(&mut captures,InputMode::Burst,events.into_iter().map(|event|(event,false)).collect(),false).await;
        finish.send(terminator).unwrap();
        while let Some(canvas)=captures.next().await {driver.frames.push(FrameGrid::from_canvas(&canvas).unwrap());}
    }).await.expect("capture must process its scenario and reach quota's quit boundary");
    assert_eq!(*outcome.lock().unwrap(),Some(crate::QuotaOutcome::Quit),"the capture terminator must reach the application's own outcome");
    driver.frames.iter().map(|grid|{let mut frame=(0..grid.height).map(|row|grid.row_text(row)).collect::<Vec<_>>().join("\n");frame.push('\n');frame}).collect()
}

async fn render_quota_navigation_until(
    view_model: QuotaStatusViewModel,
    width: usize,
    height: usize,
    events: Vec<TerminalEvent>,
    marker: &str,
) -> Vec<String> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model, width, height));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(stream));
    tokio::pin!(frames);
    let mut events = Some(events);
    let mut completed = false;
    let mut output = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            if let Some(events) = events.take() {
                for event in events {
                    sender.send(event).unwrap();
                }
            } else if !completed && frame.contains(marker) {
                completed = true;
                sender
                    .send(TerminalEvent::Key(KeyEvent::new(
                        KeyEventKind::Press,
                        KeyCode::Char('q'),
                    )))
                    .unwrap();
            }
            output.push(frame);
        }
    })
    .await;
    assert!(
        result.is_ok() && completed,
        "navigation must reveal {marker} before exit: {output:?}"
    );
    output
}

pub(crate) fn assert_quota_golden(name: &str, actual: &str) {
    let normalized = normalize_quota_golden(actual);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/quota")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_QUOTA_GOLDENS").as_deref() == Some(std::ffi::OsStr::new("1")) {
        std::fs::create_dir_all(path.parent().expect("golden path should have a parent"))
            .unwrap_or_else(|error| panic!("quota golden directory should be writable: {error}"));
        std::fs::write(&path, &normalized)
            .unwrap_or_else(|error| panic!("quota golden should be writable: {error}"));
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "missing quota golden {} ({error}); run with UPDATE_QUOTA_GOLDENS=1",
            path.display()
        )
    });
    assert_eq!(
        normalized,
        expected,
        "quota golden drift: {}",
        path.display()
    );
}

fn normalize_quota_golden(actual: &str) -> String {
    let mut normalized = String::with_capacity(actual.len());
    let mut characters = actual.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.peek() == Some(&'[') {
            characters.next();
            for sequence_character in characters.by_ref() {
                if ('@'..='~').contains(&sequence_character) {
                    break;
                }
            }
            continue;
        }
        if matches!(
            character,
            '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'
        ) {
            normalized.push('◌');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

fn meaningful_line_count(text: &str) -> usize {
    text.lines().count()
}

fn visible_quota_account_count(text: &str) -> usize {
    text.lines()
        .filter(|line| line.contains("acct") && !line.contains("Selected account"))
        .count()
}

fn capture_dir() -> PathBuf {
    let dir = std::env::var_os("CODEX_ROUTER_CAPTURE_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/ux-proof/production"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("capture dir should be writable: {error}"));
    dir
}

fn write_capture_pair(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(format!("{name}.txt")), text)
        .unwrap_or_else(|error| panic!("text capture should write: {error}"));
    std::fs::write(dir.join(format!("{name}.svg")), terminal_svg(name, text))
        .unwrap_or_else(|error| panic!("svg capture should write: {error}"));
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
}

fn render_quota_static_capture(
    view_model: QuotaStatusViewModel,
    width: usize,
    ansi: bool,
) -> String {
    let mut output = Vec::new();
    write_quota_status_view(
        &mut output,
        QuotaStatusViewModel {
            width,
            ..view_model
        },
        ansi,
    )
    .unwrap_or_else(|error| panic!("quota status should render: {error}"));
    String::from_utf8(output)
        .unwrap_or_else(|error| panic!("quota status should render utf8: {error}"))
}

fn has_quota_sidecar_details(text: &str) -> bool {
    text.lines()
        .any(|line| line.matches('┌').count() >= 2 && line.matches('┐').count() >= 2)
}

fn quota_cells_from_column(line: &str, start: usize) -> String {
    let mut column = 0;
    line.chars()
        .filter(|character| {
            let keep = column >= start;
            column += unicode_width::UnicodeWidthChar::width(*character).unwrap_or(0);
            keep
        })
        .collect()
}
fn quota_selected_panel_text(frame: &str) -> String {
    let heading = frame
        .lines()
        .find(|line| line.contains("Selected account"))
        .expect("selected panel heading");
    let start = unicode_width::UnicodeWidthStr::width(
        heading
            .get(..heading.find("Selected account").unwrap())
            .expect("matched text starts at UTF-8 boundary"),
    );
    frame
        .lines()
        .map(|line| quota_cells_from_column(line, start))
        .collect::<Vec<_>>()
        .join("\n")
}

fn quota_column_words(frame: &str, start: usize, width: usize) -> String {
    frame
        .lines()
        .map(|line| {
            let mut column = 0;
            line.chars()
                .filter(|character| {
                    let keep = column >= start && column < start + width;
                    column += unicode_width::UnicodeWidthChar::width(*character).unwrap_or(0);
                    keep
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn test_header_facts(usable: usize, fresh: usize) -> codex_router_quota_service::quota_display_facts::QuotaHeaderFacts {
    use codex_router_quota_service::quota_display_facts::*;
    QuotaHeaderFacts {
        route_band: "responses".to_owned(),
        pool: QuotaPoolCensus {
            usable,
            ..Default::default()
        },
        freshness: QuotaFreshnessCensus {
            fresh,
            ..Default::default()
        },
        observed_clients: None,
        credential_notice: None,
    }
}

fn quota_selected_panel_has_facts(frame: &str, account: &str, status: &str, reason: &str) -> bool {
    let panel = quota_selected_panel_text(frame);
    let words = panel.split_whitespace().collect::<Vec<_>>().join(" ");
    panel
        .lines()
        .any(|line| line.trim_matches(['│', ' ']) == account)
        && words.contains(&format!("Status {status}"))
        && words.contains(&format!("Reason {reason}"))
}
