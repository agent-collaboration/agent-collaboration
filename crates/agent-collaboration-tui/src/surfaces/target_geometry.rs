//! Scoped consumers observe natural target geometry without reversing layer ownership.
use super::focus_model::TargetId;
use iocraft::{ComponentDrawer, Hook, prelude::*};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct TargetGeometry {
    pub target: Option<TargetId>,
    pub top: i16,
    pub height: u16,
    pub focused: bool,
}
#[derive(Clone)]
pub(crate) struct TargetGeometryObserver(
    Arc<dyn Fn(TargetGeometry, Option<TargetGeometry>) + Send + Sync>,
);
impl TargetGeometryObserver {
    pub(crate) fn new(
        observe: impl Fn(TargetGeometry, Option<TargetGeometry>) + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(observe))
    }
}
struct GeometryObservation {
    observer: Option<TargetGeometryObserver>,
    target: Option<TargetId>,
    focused: bool,
    previous_focus: Option<TargetGeometry>,
}
impl Hook for GeometryObservation {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let geometry = TargetGeometry {
            target: self.target.clone(),
            top: drawer.canvas_position().y,
            height: drawer.size().height,
            focused: self.focused,
        };
        if let Some(observer) = &self.observer {
            (observer.0)(geometry.clone(), self.previous_focus.clone());
        }
        self.previous_focus = geometry.focused.then_some(geometry);
    }
}
pub(crate) fn use_geometry_observer(
    hooks: &mut Hooks<'_, '_>,
    observer: Option<TargetGeometryObserver>,
    target: Option<TargetId>,
    focused: bool,
) {
    let hook = hooks.use_hook(|| GeometryObservation {
        observer: observer.clone(),
        target: target.clone(),
        focused,
        previous_focus: None,
    });
    hook.observer = observer;
    hook.target = target;
    hook.focused = focused;
}
