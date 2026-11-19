use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{field} {requirement}")]
pub struct ValueValidationError {
    pub field: &'static str,
    pub requirement: &'static str,
}

// Preserve message-board::board_metadata's UTF-8 bounds and control policy.
macro_rules! bounded_text {
    ($name:ident, $min:expr, $max:expr, $trim:expr, $controls:expr) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = ValueValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let value = if $trim {
                    value.trim().to_owned()
                } else {
                    value
                };
                if !($min..=$max).contains(&value.len()) {
                    return Err(ValueValidationError {
                        field: stringify!($name),
                        requirement: "is outside its UTF-8 byte bound",
                    });
                }
                if $controls
                    && value.chars().any(|character| {
                        character <= '\u{001f}' && !matches!(character, '\n' | '\t')
                    })
                {
                    return Err(ValueValidationError {
                        field: stringify!($name),
                        requirement: "contains a forbidden control character",
                    });
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
bounded_text!(ResourceName, 1, 256, true, false);
bounded_text!(SearchQuery, 1, 256, true, false);
bounded_text!(IssueTitle, 1, 256, true, true);
bounded_text!(Description, 0, 16 * 1024, false, false);

/// A description that must hold non-whitespace text: backlog items, areas and
/// projects. Stored rows cannot deserialize without one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequiredDescription(String);
impl TryFrom<String> for RequiredDescription {
    type Error = ValueValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() || value.len() > 16 * 1024 {
            return Err(ValueValidationError {
                field: "description",
                requirement: "must contain non-whitespace text within 16384 UTF-8 bytes",
            });
        }
        Ok(Self(value))
    }
}
impl From<RequiredDescription> for String {
    fn from(value: RequiredDescription) -> Self {
        value.0
    }
}
impl RequiredDescription {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
bounded_text!(PostText, 1, 64 * 1024, false, true);
bounded_text!(TrailText, 1, 64 * 1024, false, true);
bounded_text!(TrailTag, 1, 256, true, true);

/// A page always returns at least one row when a matching row exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct PageLimit(std::num::NonZeroU16);
impl PageLimit {
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
    #[must_use]
    pub const fn lookahead(self) -> u32 {
        self.0.get() as u32 + 1
    }
}
impl TryFrom<u16> for PageLimit {
    type Error = ValueValidationError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        std::num::NonZeroU16::new(value)
            .map(Self)
            .ok_or(ValueValidationError {
                field: "limit",
                requirement: "must be a nonzero u16",
            })
    }
}
impl From<PageLimit> for u16 {
    fn from(value: PageLimit) -> Self {
        value.get()
    }
}

/// Durable ordering is independent of ids and time, within both hosts' integer range.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ChangePosition(u64);
impl Default for ChangePosition {
    fn default() -> Self {
        Self::ZERO
    }
}
impl ChangePosition {
    pub const ZERO: Self = Self(0);
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for ChangePosition {
    type Error = ValueValidationError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value > 9_007_199_254_740_991 {
            return Err(ValueValidationError {
                field: "position",
                requirement: "exceeds the safe integer range",
            });
        }
        Ok(Self(value))
    }
}
impl From<ChangePosition> for u64 {
    fn from(value: ChangePosition) -> Self {
        value.0
    }
}

#[cfg(test)]
#[path = "validated_values_test.rs"]
mod tests;
