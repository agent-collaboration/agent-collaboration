//! The kinds of provider session setting the setting machine tracks.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProviderSettingKind {
    Mode,
    Model,
    Effort,
    Fast,
}

impl ProviderSettingKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mode => "mode",
            Self::Model => "model",
            Self::Effort => "effort",
            Self::Fast => "fast",
        }
    }
}
