#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{
            HelpContent, HelpContextLine, HelpControl, HelpInputConvention, ScrollHandle,
            button_target,
        },
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::SurfaceModel,
    };
    use iocraft::prelude::*;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    const ACCOUNT: &str = "東京 OpenAI Research Account Alpha Long Name [alpha-tag]";
    const SESSION_ID: &str = "00000000-0000-4000-8000-000000000014";
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[derive(Clone)]
    struct HelpFixture {
        stack: SurfaceStackHandle,
        help: HelpControl,
    }
    #[derive(Props)]
    struct NarrowHelpSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        handle: Arc<Mutex<Option<HelpFixture>>>,
    }
    #[component]
    fn NarrowHelpScene(
        props: &NarrowHelpSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, action("help"));
            *props.handle.lock().unwrap() = Some(HelpFixture {
                stack,
                help: help.clone(),
            });
            let open = ActionDef {
                id: action("help"),
                title: "Help".into(),
                toolbar_label: None,
                keys: vec!["F1".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: help.open_handler(
                    || HelpContent {
                        context: vec![
                            HelpContextLine {
                                label: "Captured account".into(),
                                value: ACCOUNT.into(),
                            },
                            HelpContextLine {
                                label: "Session ID".into(),
                                value: SESSION_ID.into(),
                            },
                            HelpContextLine {
                                label: "Workspace".into(),
                                value: "Project /remote/only/project".into(),
                            },
                            HelpContextLine {
                                label: "Captured tag".into(),
                                value: "Captured item [alpha-tag]".into(),
                            },
                            HelpContextLine {
                                label: "Spacing".into(),
                                value: "OpenAI  Research\nSecond  line".into(),
                            },
                        ],
                        conventions: vec![
                            HelpInputConvention {
                                key: "Shift+PageDown".into(),
                                description: "Scroll through the remaining account details".into(),
                            },
                            HelpInputConvention {
                                key: "Typing".into(),
                                description: "Preserve the current query and draft workspace"
                                    .into(),
                            },
                        ],
                    },
                    Handler::from(|error| panic!("{error}")),
                ),
            };
            let inspect = ActionDef {
                id: action("inspect"),
                title: "Inspect metadata".into(),
                toolbar_label: None,
                keys: Vec::new(),
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Menu { section: "Actions" },
                run: Handler::default(),
            };
            let refresh = ActionDef {
                id: action("refresh"),
                title: "Refresh account details".into(),
                toolbar_label: None,
                keys: vec![
                    "Ctrl+Shift+F9".parse().unwrap(),
                    "Ctrl+Alt+F8".parse().unwrap(),
                ],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::default(),
            };
            SurfaceModel {
                focus: FocusModel::new(vec![button_target(TargetId::for_action(&open.id), &open)])
                    .unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("narrow-help").unwrap(),
                    vec![open, inspect, refresh],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),Vec::new())]))) {OverlayHost {Text(content:"Narrow Help scene")}})
    }
    fn collect_body(grid: &FrameGrid, scroll: &ScrollHandle, lines: &mut BTreeMap<u16, String>) {
        let position = scroll.position();
        for row in 0..position.viewport_height {
            let Ok(actual) = usize::try_from(i32::from(position.viewport_top) + i32::from(row))
            else {
                continue;
            };
            if actual < grid.height {
                lines.insert(
                    position.offset + row,
                    grid.row_text(actual).replace(['│', '█'], " "),
                );
            }
        }
    }
    #[tokio::test]
    async fn help_keeps_context_words_identifiers_aliases_and_conventions_readable_at_six_grids() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in [
                (28u16, 60u16),
                (32, 24),
                (40, 24),
                (48, 24),
                (48, 12),
                (120, 36),
            ] {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(NarrowHelpScene(journal,width,height,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                        .await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    let scroll = fixture
                        .help
                        .scroll(fixture.stack.snapshot().top_id())
                        .unwrap();
                    let map = fixture.stack.hit_regions().published().unwrap();
                    let region = map
                        .targets
                        .iter()
                        .find(|region| {
                            region.surface == fixture.stack.snapshot().top_id()
                                && region.target == fixture.help.scroll_target()
                        })
                        .unwrap();
                    let minimum = region
                        .cells
                        .iter()
                        .map(|(column, _)| *column)
                        .min()
                        .unwrap();
                    let maximum = region
                        .cells
                        .iter()
                        .map(|(column, _)| *column)
                        .max()
                        .unwrap();
                    let body_width = usize::from(maximum - minimum); // one reserved scrollbar cell
                    let mut lines = BTreeMap::new();
                    collect_body(driver.frames.last().unwrap(), &scroll, &mut lines);
                    for _ in 0..=scroll.position().natural_height {
                        let before = scroll.position();
                        if before.offset == before.maximum() {
                            break;
                        }
                        driver
                            .drive_checked(
                                &mut frames,
                                mode,
                                vec![(key(KeyCode::PageDown), true)],
                                true,
                                |_| {
                                    assert!(
                                        scroll.position().offset > before.offset,
                                        "a real PageDown advances the measured body"
                                    )
                                },
                            )
                            .await;
                        collect_body(driver.frames.last().unwrap(), &scroll, &mut lines);
                    }
                    assert_eq!(scroll.position().offset, scroll.position().maximum());
                    let text = lines.values().cloned().collect::<Vec<_>>().join("\n");
                    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    for fact in [
                        ACCOUNT,
                        "Captured account",
                        "Session ID",
                        "Workspace",
                        "Captured tag",
                        "Inspect metadata",
                        "Refresh account details",
                        "Scroll through the remaining account details",
                        "Preserve the current query and draft workspace",
                    ] {
                        assert!(
                            normalized.contains(fact),
                            "complete Help fact at {width}x{height}: {fact}\n{text}"
                        );
                    }
                    assert!(
                        text.lines().any(|line| line.contains("OpenAI  Research"))
                            && text.lines().any(|line| line.contains("Second  line")),
                        "original spaces and line breaks survive native token layout\n{text}"
                    );
                    for token in [
                        "[alpha-tag]",
                        "/remote/only/project",
                        "Ctrl+Shift+F9",
                        "Ctrl+Alt+F8",
                        "Shift+PageDown",
                    ] {
                        assert!(
                            token.len() > body_width
                                || text.lines().any(|line| line.contains(token)),
                            "a token that fits the full body stays on one row: {token}\n{text}"
                        );
                    }
                    if body_width >= SESSION_ID.len() {
                        assert!(
                            text.lines().any(|line| line.contains(SESSION_ID)),
                            "fitting UUID stays whole after reflow\n{text}"
                        );
                    } else {
                        assert!(
                            text.chars()
                                .filter(|character| !character.is_whitespace())
                                .collect::<String>()
                                .contains(SESSION_ID),
                            "overlong UUID may break without losing characters"
                        );
                    }
                })
                .await
                .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn help_token_layout_tracks_live_width_and_returns_to_its_exact_wide_grid() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui =
                element!(NarrowHelpScene(journal,width:120u16,height:36u16,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                    .await;
                let initial = driver.frames.last().unwrap().round_frame_text();
                let fixture = handle.lock().unwrap().clone().unwrap();
                let scroll = fixture
                    .help
                    .scroll(fixture.stack.snapshot().top_id())
                    .unwrap();
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Resize(28, 60), true)],
                        true,
                    )
                    .await;
                let mut lines = BTreeMap::new();
                collect_body(driver.frames.last().unwrap(), &scroll, &mut lines);
                for _ in 0..=scroll.position().natural_height {
                    let before = scroll.position();
                    if before.offset == before.maximum() {
                        break;
                    }
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::PageDown), true)],
                            true,
                            |_| assert!(scroll.position().offset > before.offset),
                        )
                        .await;
                    collect_body(driver.frames.last().unwrap(), &scroll, &mut lines);
                }
                let text = lines.values().cloned().collect::<Vec<_>>().join("\n");
                assert!(
                    text.chars()
                        .filter(|character| !character.is_whitespace())
                        .collect::<String>()
                        .contains(SESSION_ID),
                    "resized overlong ID retains every character\n{text}"
                );
                assert!(
                    text.lines()
                        .any(|line| line.contains("/remote/only/project"))
                        && text.lines().any(|line| line.contains("[alpha-tag]")),
                    "fitting tokens stay whole after actual resize\n{text}"
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Resize(120, 36), true)],
                        true,
                    )
                    .await;
                if scroll.position().offset > 0 {
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Home), true)], true)
                        .await;
                }
                assert_eq!(
                    driver.frames.last().unwrap().round_frame_text(),
                    initial,
                    "continuing Help returns to the exact initial wide grid"
                );
            })
            .await
            .unwrap();
        }
    }
}
