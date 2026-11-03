use crate::test_support::{
    event_driver::{EventDriver, EventJournal, SurfaceSnapshot, record_draw},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    DialogAxisSize, DialogMargins, DialogPresentation,
    actions::{ActionId, ActionRegistry, ScopeId},
    controls::list::ScrollHandle,
    dialog::DialogFrame,
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    },
};
use iocraft::prelude::*;
fn body() -> TargetId {
    TargetId::new("context-body").unwrap()
}
#[derive(Props)]
struct ContextSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    context: bool,
    width: u16,
    height: u16,
}
#[component]
fn ContextScene(props: &ContextSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("context-screen").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let scroll = ScrollHandle::new(stack);
        let with_context = props.context;
        stack.open_overlay(OverlaySpec {
                kind:OverlayKind::Dialog,
                model:SurfaceModel {
                    focus:FocusModel::new(vec![TargetSpec::new(body()).with_input(scroll.input_handler())]).unwrap(),
                    actions:ActionRegistry::new(ScopeId::new("context-dialog").unwrap(),Vec::new()).unwrap(),primary_action:None,
                },
                view:OverlayView::new(move || {
                    let context=with_context.then(||agent_collaboration_tui::dialog::PinnedContext::new("PINNED CONTEXT"));
                    element!(DialogFrame(title:"Spacing".to_owned(),padding:Some(0u16),scroll:scroll.clone(),scroll_target:body(),context) {
                        Text(content:"BODY FIRST")
                        Text(content:"BODY LAST")
                    }).into_any()
                }),
                layout:OverlayLayout::Dialog {presentation:DialogPresentation::Sized {width:DialogAxisSize::cells(48),height:DialogAxisSize::Content {maximum:Some(20)}},margins:DialogMargins {left:2,right:2,top:1,bottom:1}},
            }).unwrap();
        opened.set(true);
    }
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(Vec::new()))) {OverlayHost})
}
#[tokio::test]
async fn pinned_context_has_one_body_gap_and_content_height_counts_it_without_an_empty_context_gap()
{
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        for context in [true, false] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ContextScene(journal, width, height, context));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let dialog = driver.frames.last().unwrap().round_frame_text();
                let rows = dialog.lines().collect::<Vec<_>>();
                let header = rows
                    .iter()
                    .position(|row| {
                        row.contains(if context { "PINNED CONTEXT" } else { "Spacing" })
                    })
                    .unwrap();
                let first = rows
                    .iter()
                    .position(|row| row.contains("BODY FIRST"))
                    .unwrap();
                let last = rows
                    .iter()
                    .position(|row| row.contains("BODY LAST"))
                    .unwrap();
                assert_eq!(
                    first - header,
                    if context { 2 } else { 1 },
                    "exactly one row only when pinned context is present\n{dialog}"
                );
                if context {
                    assert!(
                        rows[header + 1]
                            .chars()
                            .all(|character| character == ' ' || character == '│'),
                        "context/body separator is blank\n{dialog}"
                    );
                }
                assert_eq!(last, first + 1, "no blank row inside the body\n{dialog}");
                assert_eq!(
                    rows.len(),
                    if context { 6 } else { 5 },
                    "content-sized height includes the separator and no excess space\n{dialog}"
                );
                assert_eq!(
                    last + 2,
                    rows.len(),
                    "final body row is visible directly above the border\n{dialog}"
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
