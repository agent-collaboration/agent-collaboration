#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        DialogAxisSize, DialogMargins, DialogPresentation, FormFields,
        actions::{
            ActionDef, ActionId, ActionRegistry, AvailabilityFn, Checked, Placement, ScopeId,
        },
        controls::{
            Button, DialogFrame, PinnedContext, ScrollHandle, ScrollReveal, SegmentedSwitch,
            button_target,
        },
        focus_model::{FocusModel, TargetId, TargetSpec},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };
    const STALE: &str = "Saved quota may remain stale until the normal quota refresh updates it.";
    const RETURN: &str = "Return to quota status.";
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn body() -> TargetId {
        TargetId::new("result-body").unwrap()
    }
    #[derive(Clone)]
    struct ResultFixture {
        stack: SurfaceStackHandle,
        scroll: ScrollHandle,
    }
    #[derive(Props)]
    struct ResultPanelProps {
        #[iocraft(required)]
        scroll: ScrollHandle,
        #[iocraft(required)]
        ready: State<bool>,
    }
    #[component]
    fn ResultPanel(props: &ResultPanelProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let context = *hooks.use_context::<SurfaceContext>();
        let (width, height) = context.stack.viewport();
        let content_width = width.saturating_sub(8).min(84);
        let rows = if props.ready.get() {
            vec![
            element!(Text(content:"Provider confirmed 1 quota window reset.",wrap:TextWrap::Wrap)).into_any().into(),
            element!(Text(content:"One reset credit was consumed.",wrap:TextWrap::Wrap)).into_any().into(),
            element!(Text(content:"")).into_any().into(),
            element!(Text(content:STALE,wrap:TextWrap::Wrap)).into_any().into(),
            element!(Text(content:RETURN,wrap:TextWrap::Wrap)).into_any().into(),
        ]
        } else {
            vec![
                element!(Text(content:"Checking provider eligibility…",wrap:TextWrap::Wrap))
                    .into_any()
                    .into(),
            ]
        };
        // Same native result subtree as the quota consumer: a switch, a wrapped
        // title and a margin-top section of typed unlabelled FormFields rows.
        element!(DialogFrame(title:if props.ready.get(){"Reset result".to_owned()}else{"Checking reset".to_owned()},scroll:props.scroll.clone(),scroll_target:body(),context:Some(PinnedContext::new("東京 OpenAI Research Account Alpha Long Name").with_tag("alpha-tag")),help_action:Some(element!(Button(id:TargetId::for_action(&action("help")),action:action("help"))).into_any())) {
            View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32,row_gap:if height<16{0}else{1}) {
                SegmentedSwitch(actions:vec![action("resets"),action("credits")])
                View(width:100pct,max_width:content_width,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                    Text(content:if props.ready.get(){"Success — reset completed"}else{"Checking provider…"},wrap:TextWrap::Wrap)
                    View(width:100pct,flex_direction:FlexDirection::Column,margin_top:1) {
                        ScrollReveal(focused:false) {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                            FormFields(stacked:content_width.saturating_sub(2)<52,children:rows)
                        }}
                    }
                }
            }
        }).into_any()
    }
    #[derive(Props)]
    struct ResultSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        handle: Arc<Mutex<Option<ResultFixture>>>,
    }
    fn open_result(
        stack: SurfaceStackHandle,
        handle: &Arc<Mutex<Option<ResultFixture>>>,
        ready: State<bool>,
    ) {
        let scroll = ScrollHandle::new(stack);
        *handle.lock().unwrap() = Some(ResultFixture {
            stack,
            scroll: scroll.clone(),
        });
        let definitions = [
            ("close", "Close", Some("Esc")),
            ("domain-close", "Close", Some("Ctrl+R")),
            ("help", "Help", Some("F1")),
            ("resets", "Resets", None),
            ("credits", "Credits", None),
        ]
        .into_iter()
        .map(|(name, title, key)| ActionDef {
            id: action(name),
            title: title.into(),
            toolbar_label: None,
            keys: key.into_iter().map(|key| key.parse().unwrap()).collect(),
            availability: AvailabilityFn::default(),
            choice: matches!(name, "resets" | "credits")
                .then(|| Checked::new(move || name == "resets")),
            placement: if matches!(name, "close" | "domain-close") {
                Placement::Toolbar
            } else {
                Placement::Hidden
            },
            run: Handler::default(),
        })
        .collect::<Vec<_>>();
        let mut definitions = definitions;
        definitions.push(ActionDef {
            id: action("finish-result"),
            title: "Finish result".into(),
            toolbar_label: None,
            keys: vec!["F5".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run: Handler::from(move |()| {
                let mut ready = ready;
                ready.set(true);
            }),
        });
        let mut targets = vec![TargetSpec::new(body()).with_input(scroll.input_handler())];
        targets.extend(
            definitions
                .iter()
                .map(|definition| button_target(TargetId::for_action(&definition.id), definition)),
        );
        stack
            .open_overlay(OverlaySpec {
                kind: OverlayKind::Dialog,
                model: SurfaceModel {
                    focus: FocusModel::new(targets).unwrap(),
                    actions: ActionRegistry::new(
                        ScopeId::new("result-dialog").unwrap(),
                        definitions,
                    )
                    .unwrap(),
                    primary_action: Some(action("close")),
                },
                view: OverlayView::new(move || {
                    element!(ResultPanel(scroll:scroll.clone(),ready)).into_any()
                }),
                layout: OverlayLayout::Dialog {
                    presentation: DialogPresentation::Sized {
                        width: DialogAxisSize::Content { maximum: Some(88) },
                        height: DialogAxisSize::Content { maximum: Some(24) },
                    },
                    margins: DialogMargins {
                        left: 2,
                        right: 2,
                        top: 1,
                        bottom: 1,
                    },
                },
            })
            .unwrap();
    }
    #[component]
    fn ResultScene(props: &ResultSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let ready = hooks.use_state(|| false);
        let handle = props.handle.clone();
        let stack = use_surface_stack(&mut hooks, |stack| {
            let open = ActionDef {
                id: action("show-result"),
                title: "Show result".into(),
                toolbar_label: None,
                keys: vec!["F4".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| open_result(stack, &handle, ready)),
            };
            SurfaceModel {
                focus: FocusModel::new(vec![button_target(TargetId::for_action(&open.id), &open)])
                    .unwrap(),
                actions: ActionRegistry::new(ScopeId::new("result-screen").unwrap(), vec![open])
                    .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),Vec::new())])) ) {OverlayHost {Button(id:TargetId::for_action(&action("show-result")),action:action("show-result"))}})
    }
    fn words(grid: &FrameGrid) -> String {
        grid.round_frame_text()
            .replace(['│', '█'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    #[tokio::test]
    async fn content_result_counts_every_wrapped_line_and_keeps_return_guidance_reachable_at_six_grids()
     {
        let facts = [
            "Success — reset completed",
            "Provider confirmed 1 quota window reset.",
            "One reset credit was consumed.",
            STALE,
            RETURN,
        ];
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in [
                (76u16, 30u16),
                (32, 24),
                (48, 12),
                (40, 24),
                (48, 24),
                (120, 36),
            ] {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(ResultScene(journal,width,height,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5),async {
                    driver.initial(&mut frames).await;
                    driver.drive(&mut frames,mode,vec![(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press,KeyCode::F(4))),true)],true).await;
                    driver.drive(&mut frames,mode,vec![(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press,KeyCode::F(5))),true)],true).await;
                    let fixture=handle.lock().unwrap().clone().unwrap();
                    let position=fixture.scroll.position();
                    let maximum=position.maximum();
                    let initial=words(driver.frames.last().unwrap());
                    if width==76 {assert_eq!(position.natural_height,10,"one tab, gap, outcome, section margin, provider, consumption, spacer, two stale lines and final return");assert_eq!(maximum,0,"the complete result fits before its bound; no avoidable scrollbar");}
                    let mut seen=facts.into_iter().filter(|fact|initial.contains(fact)).collect::<HashSet<_>>();
                    if maximum==0 {
                        let hit_map=fixture.stack.hit_regions().published().unwrap();
                        let visible_rows=hit_map.targets.iter().filter(|target|target.target==body()).flat_map(|target|target.cells.iter().map(|(_,row)|*row)).collect::<HashSet<_>>().len();
                        assert!(initial.contains(RETURN),"every final line is measured when content fits at {width}x{height}: {position:?}, visible rows {visible_rows}\n{}",driver.frames.last().unwrap().round_frame_text());
                    } else {
                        assert!(driver.frames.last().unwrap().round_frame_text().contains('█'),"overflow has a visible scrollbar");
                        for _ in 0..maximum {
                            driver.drive(&mut frames,mode,vec![(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press,KeyCode::Down)),true)],true).await;
                            let text=words(driver.frames.last().unwrap());
                            seen.extend(facts.into_iter().filter(|fact|text.contains(fact)));
                        }
                        assert_eq!(fixture.scroll.position().offset,maximum);
                        assert!(words(driver.frames.last().unwrap()).contains(RETURN),"the final line is reachable after bounded real Down events at {width}x{height}: {:?}\n{}",fixture.scroll.position(),driver.frames.last().unwrap().round_frame_text());
                    }
                    for fact in facts {assert!(seen.contains(fact),"fact remains reachable at {width}x{height}: {fact}\n{}",driver.frames.last().unwrap().round_frame_text());}
                }).await.unwrap();
            }
        }
    }
}
