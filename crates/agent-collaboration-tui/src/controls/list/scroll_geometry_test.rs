use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    frame_grid::FrameGrid,
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionRisk, AvailabilityFn, MenuSectionSpec,
        Placement, ScopeId,
    },
    controls::{
        fields::{MenuButton, button_target},
        list::{ScrollHandle, ScrollList, ScrollReveal},
        menu_overlay,
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::SurfaceModel,
    },
};
use iocraft::{ComponentDrawer, Hook, prelude::*};
use std::sync::{Arc, Mutex};
#[derive(Debug)]
struct Observation {
    painted_offset: u16,
    model_offset: u16,
    published: bool,
}
type Observations = Arc<Mutex<Vec<Observation>>>;
struct PaintObserver {
    scroll: ScrollHandle,
    stack: SurfaceStackHandle,
    observations: Observations,
}
impl Hook for PaintObserver {
    fn post_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let painted_offset = match drawer
            .canvas()
            .cell(0, 0)
            .and_then(|cell| cell.background_color)
        {
            Some(Color::Rgb { r, g: 1, b: 2 }) => u16::from(r),
            other => panic!("missing row marker: {other:?}"),
        };
        self.observations.lock().unwrap().push(Observation {
            painted_offset,
            model_offset: self.scroll.position().offset,
            published: self.stack.hit_regions().published().is_some(),
        });
    }
}
#[derive(Props)]
struct ScrollSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    #[iocraft(required)]
    observations: Observations,
}
#[component]
fn ScrollScene(props: &ScrollSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let mut scroll_ref = hooks.use_ref(|| None);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let scroll = ScrollHandle::new(stack);
        scroll_ref.set(Some(scroll.clone()));
        SurfaceModel {
            focus: FocusModel::new(vec![
                TargetSpec::new(TargetId::new("last-row").unwrap())
                    .with_input(scroll.input_handler()),
            ])
            .unwrap(),
            actions: ActionRegistry::new(ScopeId::new("scroll").unwrap(), Vec::new()).unwrap(),
            primary_action: None,
        }
    });
    let scroll = scroll_ref.read().as_ref().unwrap().clone();
    let observer = hooks.use_hook(|| PaintObserver {
        scroll: scroll.clone(),
        stack,
        observations: props.observations.clone(),
    });
    observer.scroll = scroll.clone();
    observer.stack = stack;
    observer.observations = props.observations.clone();
    element!(InputOwner(stack,width:40u16,height:6u16,quit_action:ActionId::new("quit").unwrap()) {OverlayHost {ScrollList(id:TargetId::new("last-row").unwrap(),scroll,grow:true) {
        #((0u8..40).map(|row|element!(ScrollReveal(focused:row==39) {View(width:100pct,height:1,flex_shrink:0.0_f32,background_color:Color::Rgb {r:row,g:1,b:2}) {Text(content:format!("Row {row}"))}})))
    }}})
}
#[tokio::test]
async fn focus_reveal_during_paint_keeps_the_pointer_map_invalid_until_that_position_is_drawn() {
    let observations = Observations::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ScrollScene(journal,observations:observations.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        driver.initial(&mut frames),
    )
    .await
    .unwrap();
    let observations = observations.lock().unwrap();
    assert!(
        observations
            .iter()
            .any(|frame| frame.painted_offset != frame.model_offset),
        "must exercise a pending reveal: {observations:?}"
    );
    for observation in observations.iter() {
        assert_eq!(
            observation.published,
            observation.painted_offset == observation.model_offset,
            "{observation:?}"
        );
    }
    assert!(observations.last().unwrap().published);
    assert!(driver.frames.last().unwrap().row_text(5).contains("Row 39"));
}

#[derive(Props)]
struct MenuScrollSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    width: u16,
    height: u16,
}

#[component]
fn MenuScrollScene(
    props: &MenuScrollSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let menu_id = ActionId::new("menu").unwrap();
    let stack = use_surface_stack(&mut hooks, |stack| {
        let menu = ActionDef {
            id: menu_id.clone(),
            title: "Menu".into(),
            toolbar_label: None,
            keys: vec!["F10".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            risk: ActionRisk::Ordinary,
            run: Handler::from(move |()| {
                let registry = stack.snapshot().top().actions.clone();
                stack
                    .open_overlay(
                        menu_overlay(
                            stack,
                            &registry,
                            TargetId::for_action(&ActionId::new("menu").unwrap()),
                        )
                        .unwrap(),
                    )
                    .unwrap();
            }),
        };
        let focus =
            FocusModel::new(vec![button_target(TargetId::for_action(&menu.id), &menu)]).unwrap();
        let mut actions = vec![menu];
        let sections = [
            "Scope",
            "Runtime",
            "Sort",
            "Group by",
            "Actions",
            "Filters",
            "Application",
        ];
        actions.extend((0..26).map(|index| ActionDef {
            id: ActionId::new(format!("item-{index}")).unwrap(),
            title: crate::actions::ActionTitle::dynamic(move || {
                if index == 25 {
                    "Quit".to_owned()
                } else if index == 15 {
                    "New background session…".to_owned()
                } else {
                    format!("Item {index:02}")
                }
            }),
            toolbar_label: None,
            keys: Vec::new(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Menu {
                section: sections[match index {
                    0..=3 => 0,
                    4..=6 => 1,
                    7..=8 => 2,
                    9..=14 => 3,
                    15..=19 => 4,
                    20..=23 => 5,
                    _ => 6,
                }],
            },
            risk: ActionRisk::Ordinary,
            run: Handler::default(),
        }));
        let specs = sections
            .iter()
            .enumerate()
            .map(|(index, section)| {
                let heading_action = (index < 4).then(|| {
                    let mut heading = actions[1].clone();
                    heading.id = ActionId::new(format!("heading-{index}")).unwrap();
                    heading.title = (*section).into();
                    heading.placement = Placement::Hidden;
                    let id = heading.id.clone();
                    actions.push(heading);
                    id
                });
                MenuSectionSpec {
                    section,
                    heading_action,
                }
            })
            .collect();
        SurfaceModel {
            focus,
            actions: ActionRegistry::new(ScopeId::new("menu-scrollbar").unwrap(), actions)
                .unwrap()
                .with_menu_sections(specs)
                .unwrap(),
            primary_action: None,
        }
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),stack.snapshot().top().focus.current().map(|target|target.as_str().as_bytes().to_vec()).unwrap_or_default())]))) {
        OverlayHost {View(width:100pct,height:100pct,position:Position::Relative) {
            View(position:Position::Absolute,left:1,top:1) {
                MenuButton(id:TargetId::for_action(&menu_id),action:menu_id)
            }
        }}
    })
}

fn menu_track(grid: &FrameGrid) -> (usize, usize, usize) {
    let (top, left) = grid
        .cells
        .iter()
        .enumerate()
        .find_map(|(row, cells)| {
            cells
                .iter()
                .position(|cell| cell.text == "╭")
                .map(|column| (row, column))
        })
        .unwrap();
    let right = grid.cells[top]
        .iter()
        .position(|cell| cell.text == "╮")
        .unwrap();
    let bottom = (top + 1..grid.height)
        .find(|row| grid.cell(left, *row).unwrap().text == "╰")
        .unwrap();
    (right - 1, top + 1, bottom - 1)
}

#[tokio::test]
async fn menu_last_item_navigation_paints_the_scrollbar_thumb_at_the_content_end() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in [(76u16, 30u16), (32, 24)] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(MenuScrollScene(journal, width, height));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::F(10)), true)],
                        true,
                    )
                    .await;
                let initial = driver.frames.last().unwrap();
                let (column, first, last) = menu_track(initial);
                assert_eq!(initial.cell(column, first).unwrap().text, "█");
                assert_eq!(initial.cell(column, last).unwrap().text, "│");
                let acknowledgements = driver
                    .drive(
                        &mut frames,
                        mode,
                        (0..25).map(|_| (key(KeyCode::Down), true)).collect(),
                        true,
                    )
                    .await;
                assert_eq!(
                    acknowledgements.last().unwrap().snapshot.0[0].1,
                    TargetId::for_action(&ActionId::new("item-25").unwrap())
                        .as_str()
                        .as_bytes(),
                    "real menu input reaches the final item before checking paint"
                );
                for frame in driver.frames.iter().filter(|frame| {
                    frame
                        .cells
                        .iter()
                        .flatten()
                        .any(|cell| cell.text == "╭")
                }) {
                    let (column, _, last) = menu_track(frame);
                    if frame.row_text(last).contains("Quit") {
                        assert_eq!(
                            frame.cell(column, last).unwrap().text,
                            "█",
                            "every frame with Quit at the content end has its thumb at the end"
                        );
                    }
                }
                let final_frame = driver.frames.last().unwrap();
                let (column, first, last) = menu_track(final_frame);
                assert!(final_frame.row_text(last).contains("Quit"));
                assert_eq!(
                    final_frame.cell(column, last).unwrap().text,
                    "█",
                    "menu thumb reaches the end in the same frame as Quit at {width}x{height} {mode:?}"
                );
                assert_eq!(final_frame.cell(column, first).unwrap().text, "│");
                let before_wrap = driver.frames.len();
                driver.drive_checked(&mut frames,mode,vec![(key(KeyCode::Down),true)],true,|ack|assert_eq!(ack.snapshot.0[0].1,TargetId::for_action(&ActionId::new("item-0").unwrap()).as_str().as_bytes())).await;
                let first_wrap = &driver.frames[before_wrap];
                let (column, first, last) = menu_track(first_wrap);
                assert!(
                    first_wrap.row_text(first).contains("Item 00"),
                    "menu keyboard wrap paints the new content and current thumb in its first frame at {width}x{height} {mode:?}: {}",
                    first_wrap.row_text(first)
                );
                assert_eq!(first_wrap.cell(column,first).unwrap().text,"█");
                assert_eq!(first_wrap.cell(column,last).unwrap().text,"│");
            })
            .await
            .unwrap();
        }
    }
}
