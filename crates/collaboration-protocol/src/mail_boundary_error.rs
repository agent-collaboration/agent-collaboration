//! Conversion failures carry only the field name, never caller content.
#[derive(Debug, thiserror::Error)]
#[error("invalid mail boundary field: {0}")]
pub struct MailBoundaryConversionError(pub(crate) &'static str);
