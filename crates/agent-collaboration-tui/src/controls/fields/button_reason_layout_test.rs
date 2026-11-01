use crate::{
    DialogAxisSize, DialogMargins, DialogPresentation,
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionRisk, Availability, AvailabilityFn, Placement,
        ScopeId,
    },
    controls::{
        fields::{Button, button_target},
        list::ScrollHandle,
    },
    dialog::DialogFrame,
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    },
    test_support::{
        event_driver::{EventDriver, EventJournal, record_draw},
        recording_backend::recording_terminal_render_loop,
    },
};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
const REASON: &str = "Reset is unavailable while another request is pending.";
fn action() -> ActionId {
    ActionId::new("yes").unwrap()
}
fn sibling_action() -> ActionId {
    ActionId::new("no").unwrap()
}
fn body() -> TargetId {
    TargetId::new("body").unwrap()
}
#[derive(Props)]
struct ReasonSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    width: u16,
    height: u16,
    siblings: bool,
}
#[component]
fn ReasonScene(props: &ReasonSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("screen").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let scroll = ScrollHandle::new(stack);
        let siblings = props.siblings;
        let mut definitions = vec![ActionDef {
            risk: ActionRisk::Ordinary,
            id: action(),
            title: "Yes".into(),
            toolbar_label: None,
            keys: Vec::new(),
            availability: AvailabilityFn::new(|| Availability::Unavailable {
                reason: REASON.into(),
            }),
            choice: None,
            placement: Placement::Hidden,
            run: Handler::default(),
        }];
        if siblings {
            definitions.push(ActionDef {
                risk: ActionRisk::Ordinary,
                id: sibling_action(),
                title: "No".into(),
                toolbar_label: None,
                keys: Vec::new(),
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::default(),
            });
        }
        let mut targets = vec![TargetSpec::new(body()).with_input(scroll.input_handler())];
        if siblings {
            targets.extend(
                definitions.iter().map(|definition| {
                    button_target(TargetId::for_action(&definition.id), definition)
                }),
            );
        }
        stack.open_overlay(OverlaySpec {
            kind:OverlayKind::Dialog,
            model:SurfaceModel {
                focus:FocusModel::new(targets).unwrap(),
                actions:ActionRegistry::new(ScopeId::new("button-reason").unwrap(),definitions).unwrap(),primary_action:siblings.then(sibling_action),
            },
            view:OverlayView::new(move || {
                let actions=if siblings {Vec::new()}else{vec![element!(Button(id:TargetId::for_action(&action()),action:action(),show_key:false)).into_any()]};
                element!(DialogFrame(title:"Confirm reset".to_owned(),scroll:scroll.clone(),scroll_target:body(),actions) {
                    Text(content:"Proceed?")
                    #(siblings.then(||element!(View(column_gap:1) {
                        Button(id:TargetId::for_action(&sibling_action()),action:sibling_action(),show_key:false)
                        Button(id:TargetId::for_action(&action()),action:action(),show_key:false)
                    })))
                }).into_any()
            }),
            layout:OverlayLayout::Dialog {presentation:DialogPresentation::Sized {width:DialogAxisSize::cells(88),height:DialogAxisSize::Content {maximum:None}},margins:DialogMargins {top:1,right:2,bottom:1,left:2}},
        }).unwrap();
        opened.set(true);
    }
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap()) {OverlayHost})
}

#[tokio::test]
async fn sibling_buttons_share_the_label_row_and_wrap_the_reason_inside_its_actual_allocation() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let captured = bytes.clone();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ReasonScene(journal,width,height,siblings:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config).inspect(
            move |canvas| {
                let mut serialized = Vec::new();
                canvas.write_ansi(&mut serialized).unwrap();
                *captured.lock().unwrap() = serialized;
            },
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            if width >= 76 {
                let expected: &[u8] = if width == 120 {
                    include_bytes!("test_fixtures/button_sibling_reason_120.ansi")
                } else {
                    include_bytes!("test_fixtures/button_sibling_reason_76.ansi")
                };
                assert_eq!(
                    &*bytes.lock().unwrap(),
                    expected,
                    "sibling Button wide bytes remain identical"
                );
            }
            let grid = driver.frames.last().unwrap();
            let label = (0..grid.height)
                .find(|row| grid.row_text(*row).contains("No"))
                .expect("complete No label");
            let labels = grid.row_text(label);
            assert!(
                labels.contains("Yes"),
                "complete No and Yes labels stay on their shared row at {width}x{height}: {labels}"
            );
            let yes_column = grid.cells[label]
                .iter()
                .position(|cell| cell.text == "Y")
                .unwrap();
            let lines = (label..grid.height)
                .map(|row| grid.row_text(row))
                .collect::<Vec<_>>();
            let words = lines
                .iter()
                .map(|line| line.replace('│', " "))
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                words.contains(REASON),
                "complete sibling Button reason at {width}x{height}: {words}"
            );
            let reason = (label..grid.height)
                .find(|row| grid.row_text(*row).contains("Reset is"))
                .unwrap();
            if width < 76 {
                assert!(
                    reason > label,
                    "reason wraps below the intact shared label row"
                );
                let reason_column = grid.cells[reason]
                    .iter()
                    .position(|cell| cell.text == "R")
                    .unwrap();
                assert_eq!(
                    reason_column, yes_column,
                    "wrapped reason starts under Yes, after the sibling budget"
                );
            } else {
                assert_eq!(reason, label, "fitting wide reason keeps its inline row");
            }
            let last = (reason..grid.height)
                .find(|row| grid.row_text(*row).contains("pending."))
                .unwrap();
            let bottom = (last..grid.height)
                .find(|row| grid.row_text(*row).contains('╰'))
                .unwrap();
            assert!(
                last < bottom,
                "the complete allocated reason contributes to the dialog height"
            );
        })
        .await
        .unwrap();
    }
}
#[tokio::test]
async fn disabled_button_reason_wraps_below_its_whole_label_and_is_counted_in_dialog_height() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let captured = bytes.clone();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ReasonScene(journal, width, height));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config).inspect(
            move |canvas| {
                let mut serialized = Vec::new();
                canvas.write_ansi(&mut serialized).unwrap();
                *captured.lock().unwrap() = serialized;
            },
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            if width == 120 {
                assert_eq!(
                    &*bytes.lock().unwrap(),
                    include_bytes!("test_fixtures/button_reason_wide_120x36.ansi"),
                    "wide disabled Button remains byte-identical"
                );
            }
            let grid = driver.frames.last().unwrap();
            let label = (0..grid.height)
                .find(|row| grid.row_text(*row).contains("Yes"))
                .expect("whole button label");
            let lines = (label..grid.height)
                .map(|row| grid.row_text(row))
                .collect::<Vec<_>>();
            let words = lines
                .iter()
                .map(|line| line.replace('│', " "))
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                words.contains(REASON),
                "complete disabled button reason at {width}x{height}: {words}"
            );
            let reason = (label..grid.height)
                .find(|row| grid.row_text(*row).contains("Reset is"))
                .unwrap();
            if width < 76 {
                assert!(reason > label, "a non-fitting reason moves below the label");
            } else {
                assert_eq!(
                    reason, label,
                    "wide inline reason stays on its original row"
                );
            }
            let final_reason = (reason..grid.height)
                .find(|row| grid.row_text(*row).contains("pending."))
                .unwrap();
            let bottom = (final_reason..grid.height)
                .find(|row| grid.row_text(*row).contains('╰'))
                .unwrap();
            assert!(
                final_reason < bottom,
                "content height includes every reason row"
            );
        })
        .await
        .unwrap();
    }
}
