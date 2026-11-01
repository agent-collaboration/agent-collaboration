//! Definition authority controls timing only; existing local work keeps its execution owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AutomationAuthority {
    Local,
    Central,
}
impl TryFrom<&str> for AutomationAuthority {
    type Error = &'static str;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "local" => Ok(Self::Local),
            "central" => Ok(Self::Central),
            _ => Err("invalid automation authority"),
        }
    }
}
