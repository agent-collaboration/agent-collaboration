use message_board::{IdentityValidationError, RecordId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientId(RecordId);

impl JsonSchema for ClientId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ClientId".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        RecordId::json_schema(generator)
    }
}

impl TryFrom<String> for ClientId {
    type Error = IdentityValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        RecordId::try_from(value).map(Self)
    }
}

impl From<ClientId> for String {
    fn from(value: ClientId) -> Self {
        value.0.into()
    }
}

impl ClientId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
