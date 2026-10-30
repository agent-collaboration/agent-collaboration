#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        DialogAxisSize, DialogMargins, DialogPresentation,
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{
            Button, DialogFrame, DialogSection, DialogSections, FormField, FormFields,
            ScrollHandle, button_target,
        },
        focus_model::{FocusModel, TargetId, TargetSpec},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    fn target(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    #[derive(Clone)]
    struct ContentFixture {
        stack: SurfaceStackHandle,
        scroll: ScrollHandle,
    }
    #[derive(Props)]
    struct ContentSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        overflow: bool,
        handle: Arc<Mutex<Option<ContentFixture>>>,
    }
    #[component]
    fn ContentScene(props: &ContentSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let calls = hooks.use_state(|| 0usize);
        let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
            focus: FocusModel::new(Vec::new()).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("content-screen").unwrap(), Vec::new())
                .unwrap(),
            primary_action: None,
        });
        let scroll = hooks.use_ref(|| ScrollHandle::new(stack)).read().clone();
        let mut opened = hooks.use_ref(|| false);
        if !opened.get() {
            let save = ActionDef {
                toolbar_label: None,
                id: action("save"),
                title: "Save".into(),
                keys: vec!["Ctrl+Enter".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| {
                    let mut calls = calls;
                    calls.set(calls.get() + 1)
                }),
            };
            let cancel = ActionDef {
                toolbar_label: None,
                id: action("cancel"),
                title: "Cancel".into(),
                keys: vec!["Esc".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| {
                    stack.close_top();
                }),
            };
            let help = ActionDef {
                toolbar_label: None,
                id: action("help"),
                title: "Help".into(),
                keys: vec!["F1".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::default(),
            };
            let view_scroll = scroll.clone();
            let overflow = props.overflow;
            let identity_width = props.width.saturating_sub(18).min(60);
            stack.open_overlay(OverlaySpec {
                kind:OverlayKind::Dialog,
                model:SurfaceModel {
                    focus:FocusModel::new(vec![TargetSpec::new(target("body")).with_input(scroll.input_handler()),button_target(TargetId::for_action(&save.id),&save),button_target(TargetId::for_action(&cancel.id),&cancel)]).unwrap(),
                    actions:ActionRegistry::new(ScopeId::new("content-form").unwrap(),vec![save,cancel,help]).unwrap(),primary_action:Some(action("save")),
                },
                view:OverlayView::new(move ||element!(DialogFrame(title:"Content form".to_owned(),scroll:view_scroll.clone(),scroll_target:target("body"),help_action:Some(element!(Button(id:TargetId::for_action(&action("help")),action:action("help"))).into_any()),context:vec![element!(View(width:identity_width,max_width:100pct,column_gap:1,flex_shrink:0.0_f32) {View(min_width:0,flex_grow:1.0_f32,flex_shrink:1.0_f32,overflow:Overflow::Hidden) {Text(content:"Account Alpha",wrap:TextWrap::NoWrap)}}).into_any()]) {
                    DialogSections {
                        #((1..=4).map(|section|element!(DialogSection(heading:format!("Section {section}")) {
                            FormFields {FormField(label:"Value".to_owned()) {Text(content:format!("FIELD-{section}"))}}
                            #((overflow).then(||element!(View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
                                #((0..8).map(|row|element!(Text(content:format!("section-{section}-extra-{row}")))))
                            })))
                        }).into_any()))
                    }
                }).into_any()),
                layout:OverlayLayout::Dialog {presentation:DialogPresentation::Sized {width:DialogAxisSize::Content {maximum:Some(68)},height:DialogAxisSize::Content {maximum:Some(22)}},margins:DialogMargins {top:1,right:2,bottom:1,left:2}},
            }).unwrap();
            opened.set(true);
        }
        *props.handle.lock().unwrap() = Some(ContentFixture { stack, scroll });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),calls.get().to_le_bytes().to_vec())]))) {
            OverlayHost {Text(content:format!("Screen calls {}",calls.get()))}
        })
    }
    #[tokio::test]
    async fn content_form_draws_all_four_sections_and_actions_at_wide_and_narrow_sizes() {
        for (width, height) in [(120u16, 36u16), (120, 48), (48, 24)] {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ContentScene(journal,width,height,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let frame = driver.frames.last().unwrap();
                let dialog = frame.round_frame_text();
                for needle in [
                    "Content form",
                    "Account Alpha",
                    "Section 1",
                    "FIELD-1",
                    "Section 2",
                    "FIELD-2",
                    "Section 3",
                    "FIELD-3",
                    "Section 4",
                    "FIELD-4",
                    "Save",
                    "Cancel",
                    "Help F1",
                ] {
                    assert!(
                        dialog.contains(needle),
                        "{width}x{height}: missing {needle}\n{dialog}"
                    );
                }
                assert!(
                    dialog.lines().count() < 22,
                    "a fitting content form does not fill its maximum bound"
                );
                assert!(
                    dialog.lines().next().unwrap().chars().count()
                        <= usize::from(width.saturating_sub(4)).min(68)
                );
                let fixture = handle.lock().unwrap().clone().unwrap();
                assert_eq!(
                    fixture.scroll.position().maximum(),
                    0,
                    "a fitting form has no body overflow"
                );
                let snapshot = fixture.stack.hit_regions().published().unwrap();
                for name in ["save", "cancel"] {
                    assert!(
                        !snapshot
                            .targets
                            .iter()
                            .find(|region| region.target == TargetId::for_action(&action(name)))
                            .unwrap()
                            .cells
                            .is_empty(),
                        "{name} has visible pointer cells"
                    );
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn overflowing_content_form_scrolls_its_body_and_keeps_actions_visible() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ContentScene(journal,width:120u16,height:36u16,overflow:true,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                let first = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    first.contains("FIELD-1") && first.contains("Save") && first.contains("Cancel"),
                    "{first}"
                );
                assert!(
                    fixture.scroll.position().maximum() > 0,
                    "overflow requires a positive body scroll range"
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(
                            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::End)),
                            true,
                        )],
                        true,
                    )
                    .await;
                let last = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    last.contains("section-4-extra-7")
                        && last.contains("Save")
                        && last.contains("Cancel"),
                    "{last}"
                );
                assert_eq!(
                    fixture.scroll.position().offset,
                    fixture.scroll.position().maximum()
                );
                let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
                key.modifiers = KeyModifiers::CONTROL;
                let acknowledgements = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Key(key), true)],
                        true,
                    )
                    .await;
                assert_eq!(acknowledgements[0].snapshot.0[0].1, 1usize.to_le_bytes());
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
