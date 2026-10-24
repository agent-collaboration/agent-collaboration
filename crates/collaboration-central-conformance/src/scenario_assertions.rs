//! Literal-result assertions shared by runtime scenarios.
use crate::ConformanceResult;
pub(crate) fn check(condition: bool, message: &str) -> ConformanceResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
pub(crate) fn equal<Value: std::fmt::Debug + PartialEq>(
    actual: Value,
    expected: Value,
) -> ConformanceResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("actual {actual:?}; expected {expected:?}").into())
    }
}
