use crate::{
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionRisk, ActionTitle, Availability, AvailabilityFn,
        MenuSectionSpec, Placement, ScopeId,
    },
    controls::{
        ActionProjections,
        fields::{MenuButton, button_target},
        menu_overlay,
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    },
    test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        grids::STANDARD_GRIDS,
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
use unicode_width::UnicodeWidthStr;
fn id(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
fn definition(name: &str, title: &'static str, placement: Placement, keys: &[&str]) -> ActionDef {
    ActionDef {
        id: id(name),
        title: title.into(),
        toolbar_label: None,
        keys: keys.iter().map(|key| key.parse().unwrap()).collect(),
        availability: AvailabilityFn::default(),
        choice: None,
        placement,
        risk: ActionRisk::Ordinary,
        run: Handler::default(),
    }
}
fn section(name: &'static str, heading: Option<&str>) -> MenuSectionSpec {
    MenuSectionSpec {
        section: name,
        heading_action: heading.map(id),
    }
}
fn in_menu(section: &'static str) -> Placement {
    Placement::Menu { section }
}
#[test]
fn menu_projection_orders_sections_omits_empty_groups_and_keeps_heading_action_out_of_items() {
    let registry = ActionRegistry::new(
        ScopeId::new("sections-projection").unwrap(),
        vec![
            definition("b-one", "B one", in_menu("B"), &[]),
            definition("cycle", "Scope", in_menu("Original"), &["Ctrl+S"]),
            definition("choice", "All", in_menu("Scope"), &[]),
            definition("tool", "Run", Placement::Toolbar, &[]),
            definition("b-two", "B two", in_menu("B"), &[]),
            definition("a-one", "A one", in_menu("A"), &[]),
            definition("c-one", "C one", in_menu("C"), &[]),
            definition("hidden", "Hidden", Placement::Hidden, &[]),
        ],
    )
    .unwrap()
    .with_menu_sections(vec![
        section("B", None),
        section("Scope", Some("cycle")),
        section("Empty", None),
    ])
    .unwrap();
    let projection = ActionProjections::from_registry(&registry);
    assert_eq!(
        projection
            .menu
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        vec!["tool", "b-one", "b-two", "choice", "a-one", "c-one"],
        "menu items follow the shared section projection and omit heading actions"
    );
    assert_eq!(
        projection.help.len(),
        8,
        "other projections retain the action owner"
    );
    assert_eq!(projection.hints.len(), 8);
    assert_eq!(
        projection
            .toolbar
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        vec!["tool"]
    );
}
#[derive(Clone)]
struct MenuFixture {
    stack: SurfaceStackHandle,
    available: State<bool>,
    calls: State<usize>,
}
#[derive(Props)]
struct SectionsSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    width: u16,
    height: u16,
    handle: Arc<Mutex<Option<MenuFixture>>>,
}
#[component]
fn SectionsScene(props: &SectionsSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let available = hooks.use_state(|| true);
    let calls = hooks.use_state(|| 0usize);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let mut cycle = definition("cycle", "Scope", Placement::Hidden, &["Ctrl+S"]);
        cycle.title = ActionTitle::dynamic(move || {
            format!(
                "Scope {}",
                if available.get() { "ready" } else { "blocked" }
            )
        });
        cycle.availability = AvailabilityFn::new(move || {
            if available.get() {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: "Busy".into(),
                }
            }
        });
        cycle.run = Handler::from(move |()| {
            assert_eq!(
                stack.snapshot().top_id(),
                SurfaceId::Screen,
                "heading handler runs after menu teardown"
            );
            let mut calls = calls;
            calls.set(calls.get() + 1);
        });
        let mut menu = definition("menu", "Menu", Placement::Hidden, &["F10"]);
        menu.run = Handler::from(move |()| {
            let registry = stack.snapshot().top().actions.clone();
            let overlay = menu_overlay(stack, &registry, TargetId::for_action(&id("menu")));
            assert!(
                overlay.is_ok(),
                "valid section metadata retains its referenced hidden action"
            );
            stack.open_overlay(overlay.unwrap()).unwrap();
        });
        let focus =
            FocusModel::new(vec![button_target(TargetId::for_action(&menu.id), &menu)]).unwrap();
        let actions = vec![
            definition("help", "Help", in_menu("Application"), &["F1"]),
            definition("all", "All", in_menu("Scope"), &[]),
            definition("refresh", "Refresh", in_menu("Actions"), &["F5"]),
            cycle,
            definition("local", "Local", in_menu("Scope"), &[]),
            definition("quit", "Quit", in_menu("Application"), &["F12"]),
            definition("run", "Run", Placement::Toolbar, &["F8"]),
            menu,
        ];
        SurfaceModel {
            focus,
            actions: ActionRegistry::new(ScopeId::new("menu-sections").unwrap(), actions)
                .unwrap()
                .with_menu_sections(vec![
                    section("Actions", None),
                    section("Scope", Some("cycle")),
                    section("Empty", None),
                    section("Application", None),
                ])
                .unwrap(),
            primary_action: None,
        }
    });
    *props.handle.lock().unwrap() = Some(MenuFixture {
        stack,
        available,
        calls,
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:id("quit"),on_routed:move |_|{let snapshot=stack.snapshot();journal.processed(SurfaceSnapshot(vec![(snapshot.top_id(),snapshot.top().focus.current().map_or("-",TargetId::as_str).as_bytes().to_vec()),(SurfaceId::Screen,calls.get().to_le_bytes().to_vec())]));}) {
        OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
            Text(content:format!("Calls {}",calls.get()))
            View(padding_left:1,flex_shrink:0.0_f32) {MenuButton(id:TargetId::for_action(&id("menu")),action:id("menu"))}
            View(flex_grow:1.0_f32)
            Text(content:format!("Available {}",available.get()))
        }}
    })
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn control_key(character: char) -> TerminalEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    key.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}
#[tokio::test]
async fn sectioned_menu_paints_one_heading_and_separators_without_targets_and_reaches_quit_last() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in STANDARD_GRIDS {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(SectionsScene(journal,width,height,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                let snapshot = fixture.stack.snapshot();
                let surface = snapshot.top_id();
                let expected = ["run", "refresh", "all", "local", "help", "quit"]
                    .map(|name| TargetId::for_action(&id(name)));
                assert_eq!(
                    snapshot
                        .top()
                        .focus
                        .targets()
                        .iter()
                        .filter(|target| target.can_focus())
                        .map(|target| target.id.clone())
                        .collect::<Vec<_>>(),
                    expected,
                    "only ordered action items are menu targets"
                );
                assert_eq!(snapshot.top().focus.current(), expected.first());
                let grid = driver.frames.last().unwrap();
                let panel = grid.round_frame_text();
                assert_eq!(
                    panel.matches("Scope ready").count(),
                    1,
                    "one live heading is painted\n{panel}"
                );
                assert!(
                    panel
                        .lines()
                        .any(|line| line.contains("Scope ready") && line.contains("Ctrl+S")),
                    "heading carries its action key\n{panel}"
                );
                assert!(!panel.contains("Empty"));
                let (header_row, header_text) = (0..grid.height)
                    .map(|row| (row, grid.row_text(row)))
                    .find(|(_, line)| line.contains("Scope ready"))
                    .unwrap();
                let column = header_text.get(..header_text.find("Scope ready").unwrap()).unwrap().width();
                let point = (
                    u16::try_from(column).unwrap(),
                    u16::try_from(header_row).unwrap(),
                );
                let map = fixture.stack.hit_regions().published().unwrap();
                assert!(
                    !map.targets
                        .iter()
                        .filter(|region| region.surface == surface)
                        .any(|region| expected.contains(&region.target)
                            && region.cells.contains(&point)),
                    "heading lies outside every menu item hit region"
                );
                if height > 12 {
                    assert_eq!(panel.lines().count(),12,"six items, three separators, one heading and two border rows fit without an empty tail\n{panel}");
                    assert_eq!(
                        panel
                            .lines()
                            .filter(|line| line.starts_with('│') && line.contains('─'))
                            .count(),
                        3,
                        "one thin separator between each nonempty group\n{panel}"
                    );
                }
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Down(MouseButton::Left),
                                point.0,
                                point.1,
                            )),
                            false,
                        )],
                        false,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0],
                                (surface, expected[0].as_str().as_bytes().to_vec()),
                                "heading click is a no-op"
                            )
                        },
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Down), true); 5],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0],
                                (
                                    surface,
                                    expected[ack.sequence - 2].as_str().as_bytes().to_vec()
                                ),
                                "arrows skip separators and headings"
                            )
                        },
                    )
                    .await;
                let panel = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    panel.lines().rev().nth(1).unwrap().contains("Quit"),
                    "Quit is last and fully reachable at the content edge\n{panel}"
                );
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![
                            (key(KeyCode::Up), true),
                            (key(KeyCode::Down), true),
                            (key(KeyCode::Esc), true),
                        ],
                        true,
                        |ack| {
                            if ack.sequence == 10 {
                                assert_eq!(ack.snapshot.0[0].0, SurfaceId::Screen);
                                return;
                            }
                            let name = if ack.sequence == 8 { "help" } else { "quit" };
                            assert_eq!(
                                ack.snapshot.0[0].1,
                                TargetId::for_action(&id(name)).as_str().as_bytes()
                            );
                        },
                    )
                    .await;
            })
            .await
            .unwrap();
        }
    }
}
#[tokio::test]
async fn menu_heading_uses_live_title_key_availability_and_the_original_handler() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in STANDARD_GRIDS {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(SectionsScene(journal,width,height,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                let overlay = fixture.stack.snapshot().overlays().last().unwrap().id;
                let mut available = fixture.available;
                available.set(false);
                driver.initial(&mut frames).await;
                let panel = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    panel.contains("Scope blocked"),
                    "heading title resolves current action state\n{panel}"
                );
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(control_key('s'), false)],
                        false,
                        |ack| {
                            assert_eq!(ack.snapshot.0[0].0, SurfaceId::Overlay(overlay));
                            assert_eq!(ack.snapshot.0[1].1, 0usize.to_le_bytes());
                        },
                    )
                    .await;
                available.set(true);
                driver.initial(&mut frames).await;
                assert!(fixture.stack.update_overlay_if_open(overlay, |model| {
                    let sections = model.actions.menu_sections().to_vec();
                    let mut definitions = model.actions.definitions().to_vec();
                    let heading = definitions
                        .iter_mut()
                        .find(|action| action.id == id("cycle"))
                        .unwrap();
                    heading.title = "Scope updated".into();
                    heading.keys = vec!["Ctrl+T".parse().unwrap()];
                    model.actions = ActionRegistry::new(model.actions.scope().clone(), definitions)
                        .unwrap()
                        .with_menu_sections(sections)
                        .unwrap();
                }));
                driver.initial(&mut frames).await;
                let panel = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    panel
                        .lines()
                        .any(|line| line.contains("Scope updated") && line.contains("Ctrl+T")),
                    "updated heading key comes from the live action\n{panel}"
                );
                assert!(!panel.contains("Ctrl+S"));
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(control_key('s'), false)],
                        false,
                        |ack| assert_eq!(ack.snapshot.0[1].1, 0usize.to_le_bytes()),
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(control_key('t'), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0].0,
                                SurfaceId::Screen,
                                "menu closes before the original heading action runs"
                            );
                            assert_eq!(ack.snapshot.0[1].1, 1usize.to_le_bytes());
                        },
                    )
                    .await;
                assert_eq!(
                    fixture.calls.get(),
                    1,
                    "exactly one original handler invocation"
                );
            })
            .await
            .unwrap();
        }
    }
}
