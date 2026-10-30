#[cfg(test)]
mod tests {
    use crate::support::recording_backend::recording_terminal_render_loop;
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        focus_model::{FocusModel, TargetId, TargetSpec},
        hit_regions::{HitRegion, HitRegionKind},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use iocraft::{ComponentDrawer, Hook, prelude::*};
    use std::{
        collections::BTreeSet,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    const MARKER: Color = Color::Rgb {
        r: 17,
        g: 70,
        b: 90,
    };
    #[derive(Clone, Copy, Debug)]
    enum ClipCase {
        PartialX,
        PartialY,
        HiddenX,
        HiddenY,
        Both,
        NestedBordered,
        Unclipped,
    }
    impl ClipCase {
        fn dimensions(self) -> (u16, u16, u16, u16, u16, u16, bool) {
            match self {
                Self::PartialX => (4, 2, 8, 2, 0, 0, false),
                Self::PartialY => (4, 2, 4, 6, 0, 0, false),
                Self::HiddenX => (4, 2, 8, 2, 6, 0, false),
                Self::HiddenY => (4, 2, 4, 2, 0, 4, false),
                Self::Both => (4, 2, 8, 6, 0, 0, false),
                Self::NestedBordered => (8, 5, 8, 4, 0, 0, true),
                Self::Unclipped => (8, 4, 4, 2, 0, 0, false),
            }
        }
        fn hidden_point(self, offset: u16) -> (u16, u16) {
            let (column, row) = match self {
                Self::PartialX => (8, 2),
                Self::PartialY => (3, 4),
                Self::HiddenX => (9, 2),
                Self::HiddenY => (3, 6),
                Self::Both => (8, 4),
                Self::NestedBordered => (8, 3),
                Self::Unclipped => (27, 8),
            };
            (
                if matches!(self, Self::Unclipped) {
                    column
                } else {
                    column + offset
                },
                row,
            )
        }
    }
    #[derive(Debug)]
    struct FrameRecord {
        frame: u64,
        drawn: BTreeSet<(u16, u16)>,
        registered: BTreeSet<(u16, u16)>,
    }
    type Records = Arc<Mutex<Vec<FrameRecord>>>;
    struct GeometryRecorder {
        stack: SurfaceStackHandle,
        records: Records,
    }
    impl Hook for GeometryRecorder {
        fn post_component_draw(&mut self, drawer: &mut ComponentDrawer) {
            let map = self.stack.hit_regions().published().unwrap();
            let (width, height) = self.stack.viewport();
            let canvas = drawer.canvas();
            let mut drawn = BTreeSet::new();
            for row in 0..height {
                for column in 0..width {
                    if canvas
                        .cell(
                            isize::try_from(column).unwrap(),
                            isize::try_from(row).unwrap(),
                        )
                        .is_some_and(|cell| cell.background_color == Some(MARKER))
                    {
                        drawn.insert((column, row));
                    }
                }
            }
            let registered = map
                .targets
                .iter()
                .filter(|target| target.target.as_str() == "clipped")
                .flat_map(|target| target.cells.iter().copied())
                .collect();
            self.records.lock().unwrap().push(FrameRecord {
                frame: map.frame,
                drawn,
                registered,
            });
        }
    }
    fn id(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }
    fn action_id(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    #[derive(Props)]
    struct GeometrySceneProps {
        #[iocraft(required)]
        case: ClipCase,
        #[iocraft(required)]
        records: Records,
        #[iocraft(required)]
        hits: Arc<AtomicUsize>,
        #[iocraft(required)]
        journal: EventJournal,
    }
    #[component]
    fn GeometryScene(
        props: &GeometrySceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let offset = hooks.use_state(|| 0u16);
        let stack = use_surface_stack(&mut hooks, |_| {
            let hit = ActionDef {
                id: action_id("hit"),
                title: "Hit",
                keys: vec!["Enter".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::from({
                    let hits = props.hits.clone();
                    move |()| {
                        hits.fetch_add(1, Ordering::SeqCst);
                    }
                }),
            };
            let move_action = ActionDef {
                id: action_id("move"),
                title: "Move",
                keys: vec!["m".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::from(move |()| {
                    let mut offset = offset;
                    offset.set(if offset.get() == 0 { 12 } else { 0 });
                }),
            };
            SurfaceModel {
                focus: FocusModel::new(vec![
                    TargetSpec::new(id("visible")),
                    TargetSpec::new(id("clipped")).with_activation(action_id("hit")),
                ])
                .unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("geometry").unwrap(),
                    vec![hit, move_action],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let recorder = hooks.use_hook(|| GeometryRecorder {
            stack,
            records: props.records.clone(),
        });
        recorder.stack = stack;
        recorder.records = props.records.clone();
        let journal = props.journal.clone();
        element! {
            InputOwner(stack,width:40u16,height:12u16,quit_action:action_id("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,Vec::new())]))) {
                OverlayHost { GeometryBody(case:props.case,offset:offset.get()) }
            }
        }
    }
    #[derive(Props)]
    struct GeometryBodyProps {
        #[iocraft(required)]
        case: ClipCase,
        offset: u16,
    }
    #[component]
    fn GeometryBody(props: &GeometryBodyProps) -> impl Into<AnyElement<'static>> {
        let (width, height, target_width, target_height, margin_left, margin_top, nested) =
            props.case.dimensions();
        let target=element! {
            HitRegion(target:id("clipped"),kind:HitRegionKind::Target) {
                View(width:target_width,height:target_height,margin_left,margin_top,flex_shrink:0.0_f32,background_color:MARKER) {Text(content:"C")}
            }
        }.into_any();
        element! {
            View(width:100pct,height:100pct) {
                Text(content:format!("Layout {}",props.offset))
                HitRegion(target:id("visible"),kind:HitRegionKind::Target) {View(position:Position::Absolute,left:0,top:1,width:2,height:1) {Text(content:"V")}}
                View(position:Position::Absolute,left:3+props.offset,top:2,width,height,overflow:Overflow::Hidden,
                    border_style:if nested {BorderStyle::Single}else{BorderStyle::None}) {
                    #(if nested {element!(View(width:4,height:2,overflow:Overflow::Hidden,flex_shrink:0.0_f32) {#(Some(target))}).into_any()}else{target})
                }
            }
        }
    }
    fn marker_cells(grid: &FrameGrid) -> BTreeSet<(u16, u16)> {
        grid.cells
            .iter()
            .enumerate()
            .flat_map(|(row, cells)| {
                cells
                    .iter()
                    .enumerate()
                    .filter(|(_, cell)| cell.style.background == Some(MARKER))
                    .map(move |(column, _)| {
                        (u16::try_from(column).unwrap(), u16::try_from(row).unwrap())
                    })
            })
            .collect()
    }
    fn assert_records(records: &Records, case: ClipCase) {
        let records = records.lock().unwrap();
        assert!(!records.is_empty());
        for record in records.iter() {
            assert_eq!(
                record.registered, record.drawn,
                "{case:?} frame {}",
                record.frame
            );
        }
    }
    #[tokio::test]
    async fn every_published_frame_matches_real_clipping_including_plain_views_each_axis_move_and_resize()
     {
        for case in [
            ClipCase::PartialX,
            ClipCase::PartialY,
            ClipCase::HiddenX,
            ClipCase::HiddenY,
            ClipCase::Both,
            ClipCase::NestedBordered,
            ClipCase::Unclipped,
        ] {
            let records = Records::default();
            let hits = Arc::new(AtomicUsize::new(0));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui =
                element!(GeometryScene(case,records:records.clone(),hits:hits.clone(),journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                assert_records(&records, case);
                for offset in [0, 12] {
                    if offset != 0 {
                        driver
                            .drive(
                                &mut frames,
                                InputMode::FrameSeparated,
                                vec![(
                                    TerminalEvent::Key(KeyEvent::new(
                                        KeyEventKind::Press,
                                        KeyCode::Char('m'),
                                    )),
                                    true,
                                )],
                                true,
                            )
                            .await;
                    }
                    let (column, row) = case.hidden_point(offset);
                    let before = hits.load(Ordering::SeqCst);
                    driver
                        .drive(
                            &mut frames,
                            InputMode::FrameSeparated,
                            vec![(
                                TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                    MouseEventKind::Down(MouseButton::Left),
                                    column,
                                    row,
                                )),
                                false,
                            )],
                            false,
                        )
                        .await;
                    assert_eq!(
                        hits.load(Ordering::SeqCst),
                        before,
                        "{case:?}: hidden point ({column},{row})"
                    );
                    assert_records(&records, case);
                    let painted = marker_cells(driver.frames.last().unwrap());
                    assert_eq!(painted, records.lock().unwrap().last().unwrap().drawn);
                    if let Some((column, row)) = painted.first().copied() {
                        driver
                            .drive(
                                &mut frames,
                                InputMode::FrameSeparated,
                                vec![(
                                    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                        MouseEventKind::Down(MouseButton::Left),
                                        column,
                                        row,
                                    )),
                                    false,
                                )],
                                false,
                            )
                            .await;
                        assert_eq!(
                            hits.load(Ordering::SeqCst),
                            before + 1,
                            "{case:?}: visible point"
                        );
                    }
                }
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(TerminalEvent::Resize(28, 9), true)],
                        true,
                    )
                    .await;
                assert_records(&records, case);
                assert_eq!(
                    marker_cells(driver.frames.last().unwrap()),
                    records.lock().unwrap().last().unwrap().drawn
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
