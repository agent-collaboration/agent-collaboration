use crate::DoArgumentValue;
use crate::database::DoArguments;

impl DoArguments {
    pub(crate) fn values(&self) -> &[DoArgumentValue] {
        &self.values
    }
}

#[cfg(test)]
#[path = "arguments_test.rs"]
mod tests;
