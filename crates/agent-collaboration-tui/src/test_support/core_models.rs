use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        surface_stack::SurfaceModel,
    },
};
use iocraft::Handler;
pub fn target(name: &str) -> TargetSpec {
    TargetSpec::new(TargetId::new(name).unwrap())
}

pub fn action(name: &str, chord: &str) -> ActionDef {
    ActionDef {
        risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
        toolbar_label: None,
        id: ActionId::new(name).unwrap(),
        title: "Open".into(),
        keys: vec![chord.parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::default(),
    }
}

pub fn surface(names: &[&str]) -> SurfaceModel {
    SurfaceModel {
        focus: FocusModel::new(names.iter().map(|name| target(name)).collect()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("screen").unwrap(), vec![]).unwrap(),
        primary_action: None,
    }
}
