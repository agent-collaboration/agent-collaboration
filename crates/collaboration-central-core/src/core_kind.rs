//! Typed transaction boundary used by both shells.
use crate::*;
pub trait CoreKind: ObjectKind {
    type Input: Clone;
    fn decode(input: WireInput) -> Result<Self::Input, CentralError>;
    fn step(
        tx: &mut dyn SqlTx,
        input: Self::Input,
        now: RegistryTime,
        ids: IdBatch,
    ) -> Result<Vec<Self::Effect>, CentralError>;
    fn pack(effects: Vec<Self::Effect>) -> ObjectEffects;
    fn unpack(effects: ObjectEffects) -> Result<Vec<Self::Effect>, CentralError>;
}
macro_rules! core_kind {
    ($kind:ident,$variant:ident,$input:ident,$reply:ident,$step:ident) => {
        impl CoreKind for $kind {
            type Input = $input;
            fn decode(input: WireInput) -> Result<Self::Input, CentralError> {
                Ok(match input {
                    WireInput::Call {
                        from,
                        call_id,
                        call,
                    } => match *call {
                        WireCall::$variant(call) => $input::Call {
                            from,
                            call_id,
                            call: Box::new(call),
                        },
                        _ => return Err(CentralError::ReplyContract),
                    },
                    WireInput::CallResult {
                        source,
                        call_id,
                        result,
                        carry,
                    } if source == Self::KIND => $input::CallResult {
                        call_id,
                        result: Box::new($reply::try_from(*result)?),
                        carry,
                    },
                    WireInput::CallFailed {
                        source,
                        call_id,
                        carry,
                    } if source == Self::KIND => $input::CallFailed { call_id, carry },
                    WireInput::Alarm { face } if face == Self::KIND => $input::Alarm,
                    _ => return Err(CentralError::ReplyContract),
                })
            }
            fn step(
                tx: &mut dyn SqlTx,
                input: Self::Input,
                now: RegistryTime,
                ids: IdBatch,
            ) -> Result<Vec<Self::Effect>, CentralError> {
                $step(tx, input, now, ids)
            }
            fn pack(effects: Vec<Self::Effect>) -> ObjectEffects {
                ObjectEffects::$variant(effects)
            }
            fn unpack(effects: ObjectEffects) -> Result<Vec<Self::Effect>, CentralError> {
                match effects {
                    ObjectEffects::$variant(values) => Ok(values),
                    _ => Err(CentralError::ReplyContract),
                }
            }
        }
    };
}
impl TryFrom<WireResult> for NoReply {
    type Error = CentralError;
    fn try_from(_value: WireResult) -> Result<Self, Self::Error> {
        Err(CentralError::ReplyContract)
    }
}
core_kind!(
    RegistryKind,
    Registry,
    RegistryInput,
    RegistryReply,
    step_registry
);
core_kind!(CopyKind, Copy, CopyInput, CopyReply, step_copy);
core_kind!(FleetKind, Fleet, FleetInput, FleetReply, step_fleet);
core_kind!(
    DeliveryKind,
    Delivery,
    DeliveryInput,
    NoReply,
    step_delivery
);
impl CoreKind for SessionKind {
    type Input = SessionInput;
    fn decode(input: WireInput) -> Result<Self::Input, CentralError> {
        Ok(match input {
            WireInput::Session(event) => event.try_into()?,
            WireInput::Call { call, call_id, .. } => match *call {
                #[cfg(feature = "conformance")]
                WireCall::Session(SessionCall::ConformanceClose { reason }) => {
                    SessionInput::ConformanceClose { call_id, reason }
                }
                _ => {
                    let _ = call_id;
                    return Err(CentralError::ReplyContract);
                }
            },
            WireInput::CallResult {
                source: CallerKind::Session,
                call_id,
                result,
                carry,
            } => SessionInput::CallResult {
                call_id,
                result: Box::new(SessionReply::try_from(*result)?),
                carry,
            },
            WireInput::CallFailed {
                source: CallerKind::Session,
                call_id,
                carry,
            } => SessionInput::CallFailed { call_id, carry },
            WireInput::Alarm {
                face: CallerKind::Session,
            } => SessionInput::Alarm,
            _ => return Err(CentralError::ReplyContract),
        })
    }
    fn step(
        tx: &mut dyn SqlTx,
        input: Self::Input,
        now: RegistryTime,
        ids: IdBatch,
    ) -> Result<Vec<Self::Effect>, CentralError> {
        step_session(tx, input, now, ids)
    }
    fn pack(effects: Vec<Self::Effect>) -> ObjectEffects {
        ObjectEffects::Session(effects)
    }
    fn unpack(effects: ObjectEffects) -> Result<Vec<Self::Effect>, CentralError> {
        match effects {
            ObjectEffects::Session(values) => Ok(values),
            _ => Err(CentralError::ReplyContract),
        }
    }
}
