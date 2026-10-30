//! Consumer context and input conventions accompany the generated action list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelpContextLine {
    pub label: String,
    pub value: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelpInputConvention {
    pub key: String,
    pub description: String,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HelpContent {
    pub context: Vec<HelpContextLine>,
    pub conventions: Vec<HelpInputConvention>,
}
