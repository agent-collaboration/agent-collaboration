//! Directory values use the same bounded patterns as their published schemas.
use fancy_regex::Regex;
use message_board::IdentityValidationError;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, sync::OnceLock};

// Program Design c1f9471e, "The scalars, exactly". Keep this lexical:
// URL parsers normalize bytes and accept a different language.
const DIRECTORY_ADDRESS_PATTERN: &str = concat!(
    r"^(?:https://(?:",
    r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?",
    r"(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*|\[::1\])",
    r"|http://(?:localhost|127\.0\.0\.1|\[::1\]))",
    r"(?::0*(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}",
    r"|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?",
    r"(?:/(?:[A-Za-z0-9._~!$&'()*+,;=:@/-]|%[0-9A-Fa-f]{2})*)?",
    r"$(?![\s\S])"
);

// Copied verbatim from sign-in-core's pinned Zod 4.4.3:
// zod/src/v4/core/regexes.ts, "Practical email validation". No flags.
const ZOD_EMAIL_PATTERN: &str = r"^(?!\.)(?!.*\.\.)([A-Za-z0-9_'+\-\.]*)[A-Za-z0-9_+-]@([A-Za-z0-9][A-Za-z0-9\-]*\.)+[A-Za-z]{2,}$";

fn address_pattern() -> &'static str {
    DIRECTORY_ADDRESS_PATTERN
}

fn email_pattern() -> &'static str {
    static PATTERN: OnceLock<String> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            // Preserve the pinned pattern and make the PD's ASCII and whole-input
            // requirements explicit to every schema consumer.
            format!(r"(?=[\x00-\x7F]*$(?![\s\S]))(?:{ZOD_EMAIL_PATTERN})$(?![\s\S])")
        })
        .as_str()
}

macro_rules! directory_value {
    ($name:ident, $maximum:expr, $pattern:ident, $normalize:expr) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl TryFrom<String> for $name {
            type Error = IdentityValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                static REGEX: OnceLock<Result<Regex, fancy_regex::Error>> = OnceLock::new();
                let valid = value.len() <= $maximum
                    && REGEX
                        .get_or_init(|| Regex::new($pattern()))
                        .as_ref()
                        .is_ok_and(|regex| regex.is_match(&value).unwrap_or(false));
                if valid {
                    Ok(Self(($normalize)(value)))
                } else {
                    Err(IdentityValidationError {
                        field: stringify!($name),
                        requirement: "must match the directory's bounded lexical contract",
                    })
                }
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

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                schemars::json_schema!({
                    "type": "string",
                    "pattern": $pattern(),
                    "x-maxUtf8Bytes": $maximum
                })
            }
        }
    };
}

directory_value!(HttpsUrl, 2048, address_pattern, |value: String| value);
directory_value!(ResourceUri, 2048, address_pattern, |value: String| value);
directory_value!(VerifiedEmail, 254, email_pattern, |value: String| value
    .to_ascii_lowercase());
