//! Sign-in addresses use the protocol's single lexical URL contract.
use fleet_identity_protocol::HttpsUrl;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SignInAddress(HttpsUrl);

impl TryFrom<String> for SignInAddress {
    type Error = <HttpsUrl as TryFrom<String>>::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        HttpsUrl::try_from(value).map(Self)
    }
}

impl From<SignInAddress> for String {
    fn from(value: SignInAddress) -> Self {
        value.0.into()
    }
}

impl SignInAddress {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl JsonSchema for SignInAddress {
    fn schema_name() -> Cow<'static, str> {
        "SignInAddress".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        HttpsUrl::json_schema(generator)
    }
}
