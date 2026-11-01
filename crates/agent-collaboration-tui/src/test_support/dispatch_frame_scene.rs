//! Integration-only input/receipt fixture; transport recording is shared separately.
#[cfg(all(feature = "test-support", debug_assertions))]
use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        surface_handle::use_surface_stack,
        surface_stack::SurfaceModel,
    },
};
use iocraft::prelude::*;
#[derive(Props)]
pub struct DispatchSceneProps {
    pub on_routed: Handler<()>,
    #[cfg(all(feature = "test-support", debug_assertions))]
    pub sink: Option<InputDispatchReceiptSink>,
}
#[component]
pub fn DispatchScene(
    props: &DispatchSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let value = hooks.use_state(|| 0usize);
    let quitting = hooks.use_state(|| false);
    if quitting.get() {
        hooks.use_context_mut::<SystemContext>().exit();
    }
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(vec![TargetSpec::new(TargetId::new("body").unwrap())]).unwrap(),
        actions: ActionRegistry::new(
            ScopeId::new("dispatch-frames").unwrap(),
            vec![
                ActionDef {
                    id: ActionId::new("change").unwrap(),
                    title: "Change".into(),
                    toolbar_label: None,
                    keys: vec!["F8".parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Hidden,
                    run: Handler::from(move |()| {
                        let mut value = value;
                        value.set(value.get() + 1);
                    }),
                },
                ActionDef {
                    id: ActionId::new("quit").unwrap(),
                    title: "Quit".into(),
                    toolbar_label: None,
                    keys: vec!["Ctrl+C".parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Hidden,
                    run: Handler::from(move |()| {
                        let mut quitting = quitting;
                        quitting.set(true);
                    }),
                },
            ],
        )
        .unwrap(),
        primary_action: None,
    });
    let on_routed = props.on_routed.clone();
    let root = element!(InputOwner(stack,width:40u16,height:8u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|on_routed(())) {
        Text(content:format!("Value {}",value.get()))
    });
    #[cfg(all(feature = "test-support", debug_assertions))]
    let root = {
        let mut root = root;
        root.props.dispatch_receipt = props.sink.clone();
        root
    };
    root
}
