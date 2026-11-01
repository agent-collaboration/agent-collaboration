//! The R15 factory mounts quota's real component; it never rebuilds its controls.
use super::super::QuotaFixtureController;
use crate::quota_control_context::{QuotaControlContext, QuotaDialogKind};
use crate::quota_status_component::QuotaStatusComponent;
use crate::quota_test_support::{
    control_kind_contract::{ControlInstanceRequest, ControlKind, ObservedControlStack},
    event_driver::{EventJournal, SurfaceSnapshot, record_draw},
};
use agent_collaboration_tui::surfaces::{
    surface_handle::SurfaceStackHandle,
    surface_stack::{OverlayKind, SurfaceId},
};
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, ResetSessionIntent, ResetSessionPorts, ResetWorkflowSnapshot,
    WorkflowPhase,
};
use iocraft::prelude::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub(super) fn build(
    request: ControlInstanceRequest,
    journal: EventJournal,
    observed: ObservedControlStack,
) -> AnyElement<'static> {
    assert_eq!(request.screen, "quota");
    assert!(
        !matches!(
            request.kind,
            ControlKind::TextField | ControlKind::FilterField | ControlKind::DialogKeyHints
        ),
        "Lead01a19627: these kinds are not on quota; the combo query belongs to ComboBox"
    );
    element!(QuotaControlContractRoot(request:Some(request),journal:Some(journal),observed:Some(observed))).into_any()
}

pub(super) fn build_committing_switch(
    request: ControlInstanceRequest,
    journal: EventJournal,
    observed: ObservedControlStack,
) -> AnyElement<'static> {
    assert_eq!(request.screen, "quota");
    assert_eq!(request.kind, ControlKind::SegmentedSwitch);
    let snapshot = crate::quota_browse_presentation_test::quota_ux_clarity_test::quota_ux_reset_test::clarity_reset_snapshot(WorkflowPhase::Committing);
    element!(QuotaControlContractRoot(request:Some(request),journal:Some(journal),observed:Some(observed),reset_snapshot:Some(snapshot))).into_any()
}

#[derive(Props)]
struct QuotaControlContractRootProps {
    request: Option<ControlInstanceRequest>,
    journal: Option<EventJournal>,
    observed: Option<ObservedControlStack>,
    reset_snapshot: Option<ResetWorkflowSnapshot>,
}

#[component]
fn QuotaControlContractRoot(
    props: &QuotaControlContractRootProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let request = props.request.unwrap();
    let journal = props.journal.clone().unwrap();
    record_draw(&mut hooks, &journal);
    let controller = hooks.use_memo(QuotaFixtureController::default, ());
    // The existing authority-free reset port supplies a real disabled Yes state.
    // No worker/provider executes these recorded presentation-originated intents.
    let snapshot = props.reset_snapshot.clone().unwrap_or_else(|| crate::quota_browse_presentation_test::quota_ux_clarity_test::quota_ux_reset_test::confirmation_selection_snapshot(ConfirmationSelection::No, false));
    let reset = hooks.use_memo(
        move || {
            let (ports, intents, snapshots) = ResetSessionPorts::test_channels(snapshot);
            (
                ports.intent_sender,
                ports.snapshot_receiver,
                Arc::new(Mutex::new(intents)),
                snapshots,
            )
        },
        (),
    );
    let reset_connected = request.kind == ControlKind::Button || props.reset_snapshot.is_some();
    let intent_receiver = reset.2.clone();
    let observed_controller = controller.clone();
    let routed = Handler::from(move |_: Option<crate::QuotaOutcome>| {
        // Drain only intents synchronously accepted by the just-finished input.
        // This is a typed channel read, not a timer or a rendered-text inference.
        while let Ok(intent) = intent_receiver.lock().unwrap().try_recv() {
            journal.log_handler(match intent {
                ResetSessionIntent::BeginInspection {
                    account_id,
                    active_credential_generation,
                    ..
                } => format!(
                    "begin-inspection:{}:{active_credential_generation}",
                    account_id.as_str()
                ),
                ResetSessionIntent::SelectNo => "select-no".to_owned(),
                ResetSessionIntent::SelectYes => "select-yes".to_owned(),
                ResetSessionIntent::Cancel => "cancel-reset".to_owned(),
                other => format!("unexpected:{other:?}"),
            });
        }
        let live = observed_controller
            .0
            .lock()
            .unwrap()
            .live
            .clone()
            .expect("actual quota mount");
        journal.processed(SurfaceSnapshot(vec![(
            SurfaceId::Screen,
            serde_json::to_vec(&model_snapshot(request.kind, &live.context, live.stack)).unwrap(),
        )]));
    });
    let mut view_model = crate::quota_browse_presentation_test::quota_two_account_view_model();
    if props.reset_snapshot.is_some() {
        // A real captured account identity measures the wide dialog. Smaller
        // viewports still exercise the shared switch's hidden projection.
        view_model.rows[0].account =
            "alpha account with a long captured identity for the wide committing dialog".to_owned();
    }
    element!(QuotaStatusComponent(
        view_model,
        width:usize::from(request.viewport.0),height:usize::from(request.viewport.1),
        fixture_controller:Some(controller),
        surface_stack_observer:Some(props.observed.as_ref().unwrap().observer()),
        routed_outcome_observer:Some(routed),
        reset_intent_sender:reset_connected.then(||reset.0.clone()),
        reset_snapshot_receiver:reset_connected.then(||reset.1.clone()),
    ))
}

fn dialog_name(context: &QuotaControlContext) -> Option<&'static str> {
    context
        .dialog
        .read()
        .as_ref()
        .map(|session| match &session.kind {
            QuotaDialogKind::Account(_) => "account",
            QuotaDialogKind::Pool => "pool",
            QuotaDialogKind::Filters => "filters",
            QuotaDialogKind::Options => "options",
            QuotaDialogKind::Floor => "floor",
        })
}

fn value(choice: &agent_collaboration_tui::controls::combo::ComboBoxState) -> Option<String> {
    choice.value().map(|key| key.as_str().to_owned())
}

fn model_snapshot(
    kind: ControlKind,
    context: &QuotaControlContext,
    stack: SurfaceStackHandle,
) -> Value {
    match kind {
        ControlKind::Button => json!({
            "dialog":dialog_name(context),
            "choice":context.reset_snapshot.read().as_ref().map(|snapshot|format!("{:?}",snapshot.confirmation_selection())),
            "yesReason":stack.snapshot().top().actions.presentations().into_iter().find(|action|action.id.as_str()=="quota:reset-yes").and_then(|action|match action.availability {agent_collaboration_tui::actions::Availability::Unavailable {reason}=>Some(reason),agent_collaboration_tui::actions::Availability::Available=>None}),
        }),
        ControlKind::TextField | ControlKind::FilterField | ControlKind::DialogKeyHints => {
            panic!("not on quota; applicability checked before mounting")
        }
        ControlKind::ComboBox => {
            let snapshot = stack.snapshot();
            let dropdown = snapshot
                .overlays()
                .last()
                .and_then(|overlay| match &overlay.kind {
                    OverlayKind::Dropdown { field } => Some(field.as_str()),
                    _ => None,
                });
            let active = dropdown.and_then(|field| {
                let choice = match field {
                    "action:quota:provider-field" => context.provider_choice,
                    "action:quota:disabled-field" => context.disabled_choice,
                    "action:quota:group-field" => context.group_choice,
                    "action:quota:credit-field" => context.credit_choice,
                    _ => return None,
                };
                let choice = choice.read();
                Some((
                    choice.query().text().to_owned(),
                    choice.query().cursor(),
                    choice.highlight().map(|key| key.as_str().to_owned()),
                ))
            });
            json!({"provider":value(&context.provider_choice.read()),"disabled":value(&context.disabled_choice.read()),"group":value(&context.group_choice.read()),"credit":value(&context.credit_choice.read()),"dropdown":dropdown,"activeQuery":active.as_ref().map(|(query,_,_)|query),"activeCursor":active.as_ref().map(|(_,cursor,_)|cursor),"activeHighlight":active.as_ref().and_then(|(_,_,highlight)|highlight.as_ref()),"draft":context.filters.read().draft.as_ref().map(|draft|draft.filters.provider.label()),"committed":context.filters.read().committed.provider.label(),"creditPhase":context.account_options.read().as_ref().and_then(|options|options.editor.as_ref().map(|editor|format!("{:?}",editor.phase))),"notice":context.notice.read().clone()})
        }
        ControlKind::SegmentedSwitch => {
            json!({"tab":context.account_options.read().as_ref().map(|options|format!("{:?}",options.tab)),"pending":context.account_options.read().as_ref().and_then(|options|options.pending_tab.map(|tab|format!("{tab:?}"))),"focused":stack.snapshot().top().focus.current().map(|target|target.as_str())})
        }
        ControlKind::ScrollingList => {
            json!({"selected":context.account_selection.read().selected().map(|key|key.as_str())})
        }
        ControlKind::FormField => {
            json!({"value":value(&context.provider_choice.read()),"draft":context.filters.read().draft.as_ref().map(|draft|draft.filters.provider.label()),"committed":context.filters.read().committed.provider.label()})
        }
    }
}
