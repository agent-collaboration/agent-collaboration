#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
        list_scene::key,
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{
            ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState, MenuButton, OptionKey,
            SelectOption, button_target, menu_overlay,
        },
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use futures_util::StreamExt;
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};

    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn anchor_target(menu: bool) -> TargetId {
        if menu {
            TargetId::for_action(&action("anchor-menu"))
        } else {
            TargetId::new("anchor-field").unwrap()
        }
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Bounds {
        left: u16,
        top: u16,
        right: u16,
        bottom: u16,
    }
    fn bounds(cells: &[(u16, u16)]) -> Bounds {
        Bounds {
            left: cells.iter().map(|cell| cell.0).min().unwrap(),
            top: cells.iter().map(|cell| cell.1).min().unwrap(),
            right: cells.iter().map(|cell| cell.0).max().unwrap(),
            bottom: cells.iter().map(|cell| cell.1).max().unwrap(),
        }
    }
    fn popup_bounds(frame: &FrameGrid) -> Bounds {
        let (left, top) = frame
            .cells
            .iter()
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|cell| cell.text == "╭")
                    .map(|column| (column, row))
            })
            .expect("native popup top-left corner");
        let right = frame.cells[top]
            .iter()
            .rposition(|cell| cell.text == "╮")
            .expect("native popup top-right corner");
        let bottom = (top..frame.height)
            .find(|row| frame.cell(left, *row).unwrap().text == "╰")
            .expect("native popup bottom-left corner");
        Bounds {
            left: u16::try_from(left).unwrap(),
            top: u16::try_from(top).unwrap(),
            right: u16::try_from(right).unwrap(),
            bottom: u16::try_from(bottom).unwrap(),
        }
    }
    fn published_anchor(stack: SurfaceStackHandle, menu: bool) -> Bounds {
        let map = stack.hit_regions().published().unwrap();
        let region = map
            .targets
            .iter()
            .find(|region| {
                region.surface == SurfaceId::Screen && region.target == anchor_target(menu)
            })
            .unwrap();
        bounds(&region.cells)
    }
    #[derive(Props)]
    struct AnchorSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        menu: bool,
        near_bottom: bool,
        handle: Arc<Mutex<Option<SurfaceStackHandle>>>,
        picks: Arc<Mutex<Vec<OptionKey>>>,
    }
    #[component]
    fn AnchorScene(props: &AnchorSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let value = hooks.use_state(|| {
            ComboBoxState::new(
                vec![
                    SelectOption::available(OptionKey::new("alpha").unwrap(), "First choice"),
                    SelectOption::available(OptionKey::new("beta").unwrap(), "Beta"),
                ],
                Some(OptionKey::new("beta").unwrap()),
            )
            .unwrap()
        });
        let stack = use_surface_stack(&mut hooks, |stack| {
            let picks = props.picks.clone();
            let control = ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                parent_scroll: None,
                field: anchor_target(false),
                value,
                on_pick: Handler::from(move |key| picks.lock().unwrap().push(key)),
            });
            let definitions = [
                ("anchor-field", "Choose", "F3"),
                ("anchor-menu", "Menu", "F10"),
                ("anchor-refresh", "Refresh", "F5"),
                ("anchor-inspect", "Inspect", "F4"),
            ]
            .into_iter()
            .map(|(name, title, chord)| ActionDef {
                id: action(name),
                title: title.into(),
                toolbar_label: None,
                keys: vec![chord.parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: if matches!(name, "anchor-field" | "anchor-menu") {
                    Placement::Hidden
                } else {
                    Placement::Toolbar
                },
                run: match name {
                    "anchor-field" => {
                        let control = control.clone();
                        Handler::from(move |()| {
                            control.open().unwrap();
                        })
                    }
                    "anchor-menu" => Handler::from(move |()| {
                        let registry = stack.snapshot().top().actions.clone();
                        let spec = menu_overlay(stack, &registry, anchor_target(true)).unwrap();
                        stack.open_overlay(spec).unwrap();
                    }),
                    _ => Handler::default(),
                },
            })
            .collect::<Vec<_>>();
            let chosen = &definitions[usize::from(props.menu)];
            SurfaceModel {
                focus: FocusModel::new(vec![button_target(anchor_target(props.menu), chosen)])
                    .unwrap(),
                actions: ActionRegistry::new(ScopeId::new("anchor-screen").unwrap(), definitions)
                    .unwrap(),
                primary_action: None,
            }
        });
        *props.handle.lock().unwrap() = Some(stack);
        let (width, height) = stack.viewport();
        let journal = props.journal.clone();
        let top = if props.near_bottom {
            height.saturating_sub(2)
        } else {
            height / 4
        };
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(Vec::new()))) {
            OverlayHost {View(width:100pct,height:100pct,position:Position::Relative) {
                Text(content:"Credit usage")
                View(position:Position::Absolute,left:width.saturating_sub(12),top,width:12) {
                    #(if props.menu {
                        element!(MenuButton(id:anchor_target(true),action:action("anchor-menu"))).into_any()
                    } else {
                        element!(ComboBox(id:anchor_target(false),value)).into_any()
                    })
                }
            }}
        })
    }
    async fn assert_placement(width: u16, height: u16, menu: bool, near_bottom: bool) {
        let (mut driver, config, journal) = EventDriver::new();
        let handle = Arc::new(Mutex::new(None));
        let raw = Arc::new(Mutex::new(Vec::new()));
        let captured = raw.clone();
        let mut ui =
            element!(AnchorScene(journal,width,height,menu,near_bottom,handle:handle.clone()));
        let mut frames = Box::pin(
            recording_terminal_render_loop(&mut ui, config)
                .inspect(move |canvas| captured.lock().unwrap().push(canvas.clone())),
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let stack = handle.lock().unwrap().unwrap();
            let anchor = published_anchor(stack, menu);
            let start = raw.lock().unwrap().len();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(if menu { KeyCode::F(10) } else { KeyCode::F(3) }), true)],
                    true,
                )
                .await;
            let raw = raw.lock().unwrap();
            let first = FrameGrid::from_canvas(raw.get(start).unwrap()).unwrap();
            let popup = popup_bounds(&first);
            eprintln!("ANCHOR {width}x{height} menu={menu} below={} anchor={anchor:?} popup={popup:?}:\n{}",!near_bottom,first.round_frame_text());
            assert!(popup.right < width && popup.bottom < height);
            if near_bottom {
                assert_eq!(popup.bottom + 1, anchor.top, "flip directly above the anchor");
            } else {
                assert_eq!(popup.top, anchor.bottom + 1, "open directly below the anchor");
            }
            let popup_width = popup.right - popup.left + 1;
            assert_eq!(popup.left, anchor.left.min(width - popup_width), "clamp horizontally without centering");
            assert!(popup.bottom < anchor.top || popup.top > anchor.bottom, "never overlap the anchor");
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn dropdown_first_frame_is_directly_below_the_published_field() {
        assert_placement(100, 24, false, false).await;
    }
    #[tokio::test]
    async fn dropdown_flips_directly_above_a_field_near_the_bottom() {
        assert_placement(48, 12, false, true).await;
    }
    #[tokio::test]
    async fn phone_dropdown_is_clamped_to_the_viewport_without_covering_its_field() {
        assert_placement(32, 24, false, false).await;
    }
    #[tokio::test]
    async fn menu_first_frame_is_directly_below_its_published_button() {
        assert_placement(100, 24, true, false).await;
    }
    #[tokio::test]
    async fn menu_flips_directly_above_a_button_near_the_bottom() {
        assert_placement(48, 12, true, true).await;
    }
    async fn assert_reanchor_after_resize(menu: bool) {
        let (mut driver, config, journal) = EventDriver::new();
        let handle = Arc::new(Mutex::new(None));
        let raw = Arc::new(Mutex::new(Vec::new()));
        let captured = raw.clone();
        let mut ui =
            element!(AnchorScene(journal,width:120u16,height:36u16,menu,handle:handle.clone()));
        let mut frames = Box::pin(
            recording_terminal_render_loop(&mut ui, config)
                .inspect(move |canvas| captured.lock().unwrap().push(canvas.clone())),
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let stack = handle.lock().unwrap().unwrap();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(if menu {KeyCode::F(10)}else{KeyCode::F(3)}), true)],
                    true,
                )
                .await;
            let before = popup_bounds(driver.frames.last().unwrap());
            let resize_start=raw.lock().unwrap().len();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(TerminalEvent::Resize(32, 24), true)],
                    true,
                )
                .await;
            let frame = driver.frames.last().unwrap();
            let popup = popup_bounds(frame);
            let anchor = published_anchor(stack, menu);
            let first_resize=FrameGrid::from_canvas(raw.lock().unwrap().get(resize_start).unwrap()).unwrap();
            let first_resize_bounds=popup_bounds(&first_resize);
            eprintln!(
                "RESIZED menu={menu} first={first_resize_bounds:?} anchor={anchor:?} settled={popup:?}:\n{}",
                frame.round_frame_text()
            );
            assert!(popup.right < 32 && popup.bottom < 24);
            assert_eq!(popup.top, anchor.bottom + 1);
            assert_ne!(
                popup.left, before.left,
                "the old position must not survive resize"
            );
            assert_eq!(
                popup.left,
                anchor.left.min(32 - (popup.right - popup.left + 1))
            );
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn open_dropdown_reanchors_to_the_fresh_field_after_resize() {
        assert_reanchor_after_resize(false).await;
    }
    #[tokio::test]
    async fn open_menu_reanchors_to_the_fresh_button_after_resize() {
        assert_reanchor_after_resize(true).await;
    }
    #[tokio::test]
    async fn a_pointer_pick_after_resize_uses_the_new_popup_cells_once() {
        let (mut driver, config, journal) = EventDriver::new();
        let handle = Arc::new(Mutex::new(None));
        let picks = Arc::new(Mutex::new(Vec::new()));
        let mut ui = element!(AnchorScene(journal,width:120u16,height:36u16,handle:handle.clone(),picks:picks.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let stack = handle.lock().unwrap().unwrap();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(3)), true)],
                    true,
                )
                .await;
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(TerminalEvent::Resize(32, 24), true)],
                    true,
                )
                .await;
            let frame = driver.frames.last().unwrap();
            let popup = popup_bounds(frame);
            let map = stack.hit_regions().published().unwrap();
            let top = stack.snapshot().top_id();
            let target = anchor_target(false).child("option:alpha");
            let point = *map
                .targets
                .iter()
                .find(|region| region.surface == top && region.target == target)
                .unwrap()
                .cells
                .first()
                .unwrap();
            assert!(
                point.0 > popup.left
                    && point.0 < popup.right
                    && point.1 > popup.top
                    && point.1 < popup.bottom
            );
            assert!(
                frame
                    .row_text(usize::from(point.1))
                    .contains("First choice")
            );
            let click = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Down(MouseButton::Left),
                point.0,
                point.1,
            ));
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(click, true)],
                    true,
                )
                .await;
            assert_eq!(
                *picks.lock().unwrap(),
                vec![OptionKey::new("alpha").unwrap()]
            );
            assert_eq!(stack.snapshot().top_id(), SurfaceId::Screen);
        })
        .await
        .unwrap();
    }
}
