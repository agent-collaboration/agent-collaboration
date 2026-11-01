use crate::test_support::{
    event_driver::{EventDriver, EventJournal, record_draw},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{ActionId, ActionRegistry, ScopeId},
    controls::list::{ScrollHandle, ScrollList, ScrollReveal},
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
