use crate::database::DoValueRef;
use sqlx_durable_object_core::{AsValue, Value as CoreValue};

impl<'r> AsValue<'r> for DoValueRef<'r> {
    fn as_value(&self) -> &'r CoreValue {
        self.0
    }
}
