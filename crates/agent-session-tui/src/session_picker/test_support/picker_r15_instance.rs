//! Mounts the production picker; observers read its actual model and routed input.
use crate::picker_control_test_support::control_kind_contract::ControlInstanceRequest;
use crate::picker_control_test_support::control_kind_contract::ObservedControlStack;
use crate::picker_control_test_support::event_driver::EventJournal;
use crate::picker_control_test_support::event_driver::SurfaceSnapshot;
use crate::picker_control_test_support::event_driver::record_draw;
use crate::session_picker::test_support::*;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use agent_collaboration_tui::surfaces::surface_stack::OverlayKind;
use agent_collaboration_tui::surfaces::surface_stack::SurfaceId;

#[derive(Props)]
pub(crate) struct PickerR15InstanceProps {
    #[iocraft(required)]
    pub instance: ControlInstanceRequest,
    #[iocraft(required)]
    pub journal: EventJournal,
    #[iocraft(required)]
    pub observed: ObservedControlStack,
}

#[component]
pub(crate) fn PickerR15Instance(
    props: &PickerR15InstanceProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let actual = hooks.use_memo(
        || {
            Arc::new(Mutex::new(
                None::<(SurfaceStackHandle, State<SessionsPickerModel>)>,
            ))
        },
        (),
    );
    let mount = actual.clone();
    let state = actual;
    let observer = props.observed.clone();
    let journal = props.journal.clone();
    let copy_journal = props.journal.clone();
    let clipboard: crate::session_picker::details::session_id_clipboard::SessionClipboardWriter =
        Arc::new(move |identity| {
            copy_journal.log_handler(format!("copy:{identity}"));
            Ok(())
        });
    let request = r15_request();
    element!(SessionsPickerComponent(request,width:usize::from(props.instance.viewport.0),height:usize::from(props.instance.viewport.1),
        session_clipboard_writer:Some(clipboard),
        surface_observer:move |(stack,model)| {observer.record(stack);*mount.lock().unwrap()=Some((stack,model));},
        input_observer:move |()| {
            let (stack,model)=state.lock().unwrap().unwrap();
            let model=model.read();
            let snapshot=stack.snapshot();
            let overlay=snapshot.overlays().iter().map(|overlay|match &overlay.kind {
                OverlayKind::Menu=>"menu".to_owned(),OverlayKind::Dialog=>"dialog".to_owned(),OverlayKind::Dropdown {field}=>format!("dropdown:{}",field.as_str()),
            }).collect::<Vec<_>>();
            let start_reason=snapshot.overlays().iter().find_map(|overlay| {
                snapshot.surface(SurfaceId::Overlay(overlay.id))?.actions.presentations().into_iter()
                    .find(|action|action.id.as_str()=="new-start")
                    .and_then(|action|match action.availability {agent_collaboration_tui::actions::Availability::Available=>None,agent_collaboration_tui::actions::Availability::Unavailable {reason}=>Some(reason)})
            });
            let screen=snapshot.surface(SurfaceId::Screen).unwrap();
            let visible=|name:&str|screen.focus.targets().iter().find(|target|target.id.as_str()==name).unwrap().can_focus();
            let bytes=serde_json::to_vec(&serde_json::json!({
                "identity":model.focused_identity().map(|identity|format!("{identity:?}")),
                "query":model.search,"root":format!("{:?}",model.root),"overlays":overlay,
                "new":model.new_session.as_ref().map(|draft|serde_json::json!({"client":format!("{:?}",draft.client),"directory":draft.directory.to_string_lossy()})),
                "start_reason":start_reason,"details":model.details_dialog.as_ref().map(|details|details.record().session_id.clone()),
                "filter_focus":snapshot.top().focus.current().is_some_and(|target|target.as_str()=="session-filter"),
                "client_focus":snapshot.top().focus.current().is_some_and(|target|target.as_str()=="new-client"),
                "scope":visible("action:scope-folder"),"new_button":visible("action:new"),
            })).unwrap();
            journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,bytes)]));
        }
    ))
}

fn r15_request() -> SessionsPickerRequest {
    let mut request = picker_request();
    request.records = vec![
        picker_record("r15-a", "Alpha", "/repo/project-a", "codex-router", "cli"),
        picker_record("r15-b", "Beta", "/repo/project-a", "codex-router", "cli"),
    ];
    request
}
