//! An action owns its title; projections resolve it from the current consumer state.
use std::{fmt, sync::Arc};

#[derive(Clone)]
pub enum ActionTitle {
    Static(&'static str),
    Dynamic(Arc<dyn Fn() -> String + Send + Sync>),
}
impl ActionTitle {
    pub fn dynamic(title: impl Fn() -> String + Send + Sync + 'static) -> Self {
        Self::Dynamic(Arc::new(title))
    }
    pub fn get(&self) -> String {
        match self {
            Self::Static(title) => (*title).to_owned(),
            Self::Dynamic(title) => title(),
        }
    }
}
impl From<&'static str> for ActionTitle {
    fn from(title: &'static str) -> Self {
        Self::Static(title)
    }
}
impl fmt::Debug for ActionTitle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static(title) => formatter.debug_tuple("Static").field(title).finish(),
            Self::Dynamic(_) => formatter.write_str("Dynamic action title"),
        }
    }
}
