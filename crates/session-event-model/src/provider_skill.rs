//! Ephemeral provider skill identity and the current session's derived inventory.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidProviderSkill {
    InvocationName,
}

impl std::fmt::Display for InvalidProviderSkill {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvocationName => "skill invocation name must be nonempty and contain no NUL",
        })
    }
}

impl std::error::Error for InvalidProviderSkill {}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SkillInvocationName(String);

impl SkillInvocationName {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidProviderSkill> {
        let value = value.into();
        if value.is_empty() || value.contains('\0') {
            return Err(InvalidProviderSkill::InvocationName);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderSkillScope {
    User,
    Repo,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSkill {
    pub invocation_name: SkillInvocationName,
    pub description: String,
    pub scope: ProviderSkillScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillRowErrorReason {
    AmbiguousName,
    UninvocableName,
    UnrecognizedMetadata,
    MalformedMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillRowError {
    pub advertised_name: String,
    pub reason: SkillRowErrorReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkillSourceError {
    UnqualifiedLaunch,
    UnestablishedClassification,
    ProviderUnavailable,
    MalformedMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkillView {
    Unknown,
    Available {
        skills: Vec<ProviderSkill>,
        errors: Vec<SkillRowError>,
    },
    Unavailable(SkillSourceError),
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SelectedSkill {
    pub invocation_name: SkillInvocationName,
}

impl SkillView {
    /// A queued selection must still identify exactly one current skill at dispatch.
    pub fn contains_selection(&self, selection: &SelectedSkill) -> bool {
        match self {
            Self::Available { skills, .. } => {
                skills
                    .iter()
                    .filter(|skill| skill.invocation_name == selection.invocation_name)
                    .count()
                    == 1
            }
            Self::Unknown | Self::Unavailable(_) => false,
        }
    }
}

#[cfg(test)]
#[path = "provider_skill_test.rs"]
mod tests;
