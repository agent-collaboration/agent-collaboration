#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{
            ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState, OptionKey, SelectOption,
            button_target,
        },
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{OverlayLayout, SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };
    fn field() -> TargetId {
        TargetId::new("filter-choice").unwrap()
    }
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    #[derive(Clone)]
    struct FilterFixture {
        stack: SurfaceStackHandle,
        value: State<ComboBoxState>,
    }
    #[derive(Props)]
    struct FilterSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        handle: Arc<Mutex<Option<FilterFixture>>>,
    }
    #[component]
    fn FilterScene(props: &FilterSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let value = hooks.use_state(|| {
            ComboBoxState::new(
                vec![
                    SelectOption::available(OptionKey::new("alpha").unwrap(), "ab Alpha"),
                    SelectOption::available(OptionKey::new("beta").unwrap(), "ab Beta"),
                ],
                Some(OptionKey::new("alpha").unwrap()),
            )
            .unwrap()
        });
        let stack = use_surface_stack(&mut hooks, |stack| {
            let control = ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                field: field(),
                value,
                on_pick: Handler::default(),
            });
            let layout = OverlayLayout::AtCells {
                left: 2,
                top: 2,
                width: props.width.saturating_sub(4).min(48),
                height: props.height.saturating_sub(4).min(8),
            };
            let open = ActionDef {
                toolbar_label: None,
                id: action("open"),
                title: "Choose".into(),
                keys: vec!["F3".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| {
                    control.open(layout).unwrap();
                }),
            };
            SurfaceModel {
                focus: FocusModel::new(vec![button_target(field(), &open)]).unwrap(),
                actions: ActionRegistry::new(ScopeId::new("filter-layout").unwrap(), vec![open])
                    .unwrap(),
                primary_action: None,
            }
        });
        *props.handle.lock().unwrap() = Some(FilterFixture { stack, value });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),value.read().query().text().as_bytes().to_vec())]))) {
            OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {Text(content:"Filter layout") ComboBox(id:field(),value)}}
        })
    }
    #[tokio::test]
    async fn dropdown_filter_label_and_typed_query_share_one_row_at_every_declared_grid() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in [(120u16, 36u16), (48, 12), (32, 24)] {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(FilterScene(journal,width,height,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![
                                (key(KeyCode::Enter), true),
                                (key(KeyCode::Char('a')), true),
                                (key(KeyCode::Char('b')), true),
                            ],
                            true,
                        )
                        .await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    assert_eq!(fixture.value.read().query().text(), "ab");
                    assert_eq!(
                        fixture.value.read().value().unwrap().as_str(),
                        "alpha",
                        "typing a query never commits a selection"
                    );
                    let frame = driver.frames.last().unwrap();
                    let dialog = frame.round_frame_text();
                    let filter_row = dialog.lines().find(|line| line.contains("Filter:"));
                    assert!(
                        filter_row.is_some_and(|line| line.contains("ab")),
                        "{width}x{height} {mode:?}: literal label and query must remain on the same row\n{dialog}"
                    );
                    let map = fixture.stack.hit_regions().published().unwrap();
                    let region = map
                        .targets
                        .iter()
                        .find(|region| {
                            region.surface == fixture.stack.snapshot().top_id()
                                && region.target == field().child("filter")
                        })
                        .unwrap();
                    let rows = region
                        .cells
                        .iter()
                        .map(|(_, row)| *row)
                        .collect::<HashSet<_>>();
                    assert_eq!(rows.len(), 1, "filter target has one visible row");
                    let suffix = "cdefghijklmnopqrstuvwxyz-END";
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::BracketedPaste(suffix.to_owned()), true)],
                            true,
                        )
                        .await;
                    assert_eq!(
                        fixture.value.read().query().text(),
                        "abcdefghijklmnopqrstuvwxyz-END"
                    );
                    let dialog = driver.frames.last().unwrap().round_frame_text();
                    assert!(
                        dialog
                            .lines()
                            .any(|line| line.contains("Filter:") && line.contains("-END")),
                        "{width}x{height} {mode:?}: the query tail and cursor must use the editor's remaining width\n{dialog}"
                    );
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }
}
