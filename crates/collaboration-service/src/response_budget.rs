//! Encoded JSON response limits shared by inventories and application operation families.

/// The most bytes of encoded JSON any operation returns, whatever transport carries it.
pub const RESULT_LIMIT_BYTES: usize = 1024 * 1024;

/// The response bound for transports that frame a result apart from its envelope.
pub const API_RESULT_BUDGET: ResultByteBudget = ResultByteBudget::new(RESULT_LIMIT_BYTES, 0);

/// The response bound an operation's result must fit.
///
/// A response is the encoded result inside its transport's envelope. Paged operations trim
/// their page until the response fits; single results that cannot fit fail. The bound belongs
/// to the caller's transport, so the same operation serves any carrier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultByteBudget {
    response_limit_bytes: usize,
    envelope_bytes: usize,
}

impl ResultByteBudget {
    /// A response of at most `response_limit_bytes`, of which `envelope_bytes` frame the result.
    #[must_use]
    pub const fn new(response_limit_bytes: usize, envelope_bytes: usize) -> Self {
        Self {
            response_limit_bytes,
            envelope_bytes,
        }
    }

    #[must_use]
    pub const fn response_limit_bytes(self) -> usize {
        self.response_limit_bytes
    }

    /// The bytes of the whole response carrying `result`, or `None` when it cannot be encoded.
    #[must_use]
    pub fn response_bytes<TResult: serde::Serialize>(self, result: &TResult) -> Option<usize> {
        serde_json::to_vec(result)
            .ok()
            .map(|bytes| bytes.len().saturating_add(self.envelope_bytes))
    }

    /// Whether the response carrying `result` fits. An unencodable result never fits.
    #[must_use]
    pub fn admits<TResult: serde::Serialize>(self, result: &TResult) -> bool {
        self.response_bytes(result)
            .is_some_and(|bytes| bytes <= self.response_limit_bytes)
    }
}

#[cfg(test)]
#[path = "result_byte_budget_tests.rs"]
mod result_byte_budget_tests;
