use serde::Serialize;

/// A state's stable tag, as named in the machine's declared transitions.
pub trait StateTag {
    fn state_tag(&self) -> &'static str;
    fn is_terminal(&self) -> bool;
}

/// An event's stable tag, as named in the machine's declared transitions.
pub trait EventTag {
    fn event_tag(&self) -> &'static str;
}

/// A domain refusal's stable tag.
pub trait RefusalTag {
    fn refusal_tag(&self) -> &'static str;
}

/// Why a transition didn't happen: the move isn't in the machine's table, the
/// domain refused it, or the event belongs to an older run of the item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum TransitionError<E> {
    NotAllowed {
        state: &'static str,
        event: &'static str,
    },
    Domain(E),
    Stale,
}

/// One row of a machine's declared transition table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transition {
    pub from: &'static str,
    pub event: &'static str,
    pub to: &'static str,
    pub terminal: bool,
}
