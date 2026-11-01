use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    frame_grid::FrameGrid,
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    controls::{
        fields::{Button, FormField, FormFields, button_target},
        list::ScrollHandle,
    },
    dialog::{
        DialogFrame, DialogSection,
        help::{HelpContent, HelpContextLine, HelpControl, HelpDialog, HelpInputConvention},
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, use_surface_stack},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    },
};
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;
fn action() -> ActionId {
    ActionId::new("help").unwrap()
}
fn body() -> TargetId {
    TargetId::new("heading-body").unwrap()
}
#[derive(Props)]
struct HeadingPanelProps {
    #[iocraft(required)]
    help: HelpControl,
    #[iocraft(required)]
    scroll: ScrollHandle,
}
#[component]
fn HeadingPanel(props: &HeadingPanelProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let help_scroll = props.help.scroll(context.surface);
    let active = help_scroll.is_some();
    let scroll = help_scroll.unwrap_or_else(|| props.scroll.clone());
    let scroll_target = if active {
        props.help.scroll_target()
    } else {
        body()
    };
    element!(DialogFrame(title:"Headings".to_owned(),scroll,scroll_target,help_action:Some(element!(Button(id:TargetId::for_action(&action()),action:action())).into_any())) {
        #(if active {element!(HelpDialog(control:props.help.clone())).into_any()}else{element!(DialogSection(heading:"Form section".to_owned()) {FormFields {FormField(label:"Client".to_owned()) {Text(content:"Codex")}}}).into_any()})
    })
}
#[derive(Props)]
struct HeadingSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
}
#[component]
fn HeadingScene(props: &HeadingSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("heading-screen").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let help = HelpControl::new(stack, action());
        let content = HelpContent {
            context: vec![HelpContextLine {
                label: "Project".into(),
                value: "/demo/project".into(),
            }],
            conventions: vec![HelpInputConvention {
                key: "Ctrl+K".into(),
                description: "Example convention".into(),
            }],
        };
        let help_action = ActionDef {
            id: action(),
            title: "Help".into(),
            toolbar_label: None,
            keys: vec!["F1".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run: help.open_handler(
                move || content.clone(),
                Handler::from(|error| panic!("{error}")),
            ),
        };
        let scroll = ScrollHandle::new(stack);
        let view_help = help;
        let view_scroll = scroll.clone();
        stack
            .open_overlay(OverlaySpec {
                kind: OverlayKind::Dialog,
                model: SurfaceModel {
                    focus: FocusModel::new(vec![
                        TargetSpec::new(body()).with_input(scroll.input_handler()),
                        button_target(TargetId::for_action(&action()), &help_action),
                    ])
                    .unwrap(),
                    actions: ActionRegistry::new(
                        ScopeId::new("heading-dialog").unwrap(),
                        vec![help_action],
                    )
                    .unwrap(),
                    primary_action: None,
                },
                view: OverlayView::new(move || {
                    element!(HeadingPanel(help:view_help.clone(),scroll:view_scroll.clone()))
                        .into_any()
                }),
                layout: OverlayLayout::CenteredCells {
                    width: 90,
                    height: 26,
                },
            })
            .unwrap();
        opened.set(true);
    }
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:120u16,height:36u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),Vec::new())]))) {OverlayHost})
}
fn assert_heading(frame: &FrameGrid, label: &str) {
    let (row, line) = (0..frame.height)
        .map(|row| (row, frame.row_text(row)))
        .find(|(_, line)| line.contains(label))
        .unwrap();
    let byte = line.find(label).unwrap();
    let column = line.get(..byte).unwrap().width();
    for offset in 0..label.len() {
        let style = &frame.cell(column + offset, row).unwrap().style;
        assert_eq!(
            style.foreground,
            Some(Color::Rgb {
                r: 156,
                g: 188,
                b: 251
            }),
            "{label}: shipped Primary colour"
        );
        assert!(style.bold, "{label}: shipped bold section heading");
        assert!(
            !style.underline,
            "{label}: a heading is not focused action text"
        );
    }
}
fn assert_field_label(frame: &FrameGrid) {
    let (row, line) = (0..frame.height)
        .map(|row| (row, frame.row_text(row)))
        .find(|(_, line)| line.contains("Client"))
        .unwrap();
    let byte = line.find("Client").unwrap();
    let column = line.get(..byte).unwrap().width();
    for offset in 0.."Client".len() {
        let style = &frame.cell(column + offset, row).unwrap().style;
        assert_eq!(
            style.foreground,
            Some(Color::Rgb {
                r: 156,
                g: 188,
                b: 251
            }),
            "Client keeps shipped Primary colour"
        );
        assert!(!style.bold, "field labels keep normal weight");
        assert!(!style.underline, "field labels are not focused action text");
    }
}
#[cfg(feature = "test-support")]
#[test]
fn test_support_exposes_the_pinned_output_stream_api() {
    let mut ui = element!(View);
    let _configured_render = ui.render_loop().stdout(std::io::sink());
}
#[tokio::test]
async fn form_field_labels_keep_shipped_primary_colour_at_normal_weight() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(HeadingScene(journal));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        assert_field_label(driver.frames.last().unwrap());
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn dialog_section_heading_uses_the_primary_bold_style_shared_by_both_shipped_screens() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(HeadingScene(journal));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        assert_heading(driver.frames.last().unwrap(), "Form section");
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
#[tokio::test]
async fn help_context_actions_and_conventions_headings_keep_shipped_primary_bold_styles() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HeadingScene(journal));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive(
                    &mut frames,
                    mode,
                    vec![(
                        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(1))),
                        true,
                    )],
                    true,
                )
                .await;
            for heading in ["Context", "Actions", "Input conventions"] {
                assert_heading(driver.frames.last().unwrap(), heading);
            }
            driver
                .drive(
                    &mut frames,
                    mode,
                    vec![(
                        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
                        true,
                    )],
                    true,
                )
                .await;
            assert_heading(driver.frames.last().unwrap(), "Form section");
            assert_field_label(driver.frames.last().unwrap());
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
