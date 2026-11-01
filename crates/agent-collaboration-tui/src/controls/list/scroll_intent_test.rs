use super::*;
use crate::{
    actions::{ActionId, ActionRegistry, ScopeId},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetSpec},
        surface_handle::use_surface_stack,
        surface_stack::{SurfaceId, SurfaceModel},
    },
    test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot},
        recording_backend::recording_terminal_render_loop,
    },
};
type Observed = Arc<Mutex<Option<ScrollHandle>>>;
fn target() -> TargetId {
    TargetId::new("body").unwrap()
}
#[derive(Props)]
struct IntentSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    #[iocraft(required)]
    observed: Observed,
}
#[component]
fn IntentScene(props: &IntentSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut retained = hooks.use_ref(|| None);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let scroll = ScrollHandle::new(stack);
        retained.set(Some(scroll.clone()));
        SurfaceModel {
            focus: FocusModel::new(vec![
                TargetSpec::new(target()).with_input(scroll.input_handler()),
            ])
            .unwrap(),
            actions: ActionRegistry::new(ScopeId::new("scroll-intent").unwrap(), Vec::new())
                .unwrap(),
            primary_action: None,
        }
    });
    let scroll = retained.read().as_ref().unwrap().clone();
    *props.observed.lock().unwrap() = Some(scroll.clone());
    let journal = props.journal.clone();
    let observed_scroll = scroll.clone();
    element!(InputOwner(stack,width:120u16,height:36u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,observed_scroll.position().offset.to_le_bytes().to_vec())]))) {Text(content:format!("Offset {}",scroll.position().offset))})
}
#[tokio::test]
async fn manual_end_preserves_offset_32_when_the_same_focused_target_changes_geometry() {
    let observed = Observed::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(IntentScene(journal,observed:observed.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        let scroll = observed.lock().unwrap().clone().unwrap();
        scroll.measure_viewport(14, 0);
        scroll.measure_content(46);
        let mut geometry = ScrollRevealTarget {
            target_id: Some(target()),
            logical_top: 0,
            heading_top: None,
            height: 46,
            viewport_height: 14,
        };
        scroll.reveal(geometry.clone());
        driver
            .drive_checked(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(
                    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::End)),
                    true,
                )],
                true,
                |ack| assert_eq!(ack.snapshot.0[0].1, 32u16.to_le_bytes()),
            )
            .await;
        assert_eq!(scroll.position().offset, 32);
        scroll.measure_content(47);
        geometry.height = 47;
        scroll.measure_target(geometry.clone());
        scroll.reveal(geometry);
        assert_eq!(
            scroll.position().offset,
            32,
            "manual End survives geometry update for the same focus"
        );
        scroll.reveal_measured(&target());
        assert_eq!(
            scroll.position().offset,
            0,
            "explicit navigation resumes focus reveal"
        );
    })
    .await
    .unwrap();
}
