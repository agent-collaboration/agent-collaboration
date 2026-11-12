use sqlx_core::arguments::Arguments;
use sqlx_core::encode::Encode;
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;
use sqlx_durable_object_core::add_argument;

use crate::{Do, DoArgumentValue};

/// The parameters bound to one query, in placeholder order.
///
/// `format_placeholder` keeps its default, `?`, which is what Durable Object storage expects.
#[derive(Debug, Default, Clone)]
pub struct DoArguments {
    pub(crate) values: Vec<DoArgumentValue>,
}

impl DoArguments {
    pub(crate) fn values(&self) -> &[DoArgumentValue] {
        &self.values
    }
}

impl Arguments for DoArguments {
    type Database = Do;

    fn reserve(&mut self, additional: usize, _size: usize) {
        self.values.reserve(additional);
    }

    fn add<'t, T>(&mut self, value: T) -> Result<(), BoxDynError>
    where
        T: Encode<'t, Do> + Type<Do>,
    {
        add_argument(&mut self.values, value)
    }

    fn len(&self) -> usize {
        self.values.len()
    }
}

#[cfg(test)]
#[path = "arguments_test.rs"]
mod tests;
