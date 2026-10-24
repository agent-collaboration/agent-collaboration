//! Decode qualified Claude skill declarations without evaluating their instruction body.
use super::skill_document::split_skill_document;
use serde::Deserialize;

/// Declared file facts only. Absence does not imply enablement or native loading.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClaudeSkillFileMetadata {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub display_version: Option<String>,
    pub has_hook_declarations: bool,
    pub user_invocable: Option<bool>,
    pub disable_model_invocation: Option<bool>,
    pub has_conditional_paths: bool,
    pub fallback: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SkillFileReadError {
    #[error("native skill frontmatter is not valid supported YAML")]
    InvalidYaml,
    #[error("native skill frontmatter delimiter is ambiguous or incomplete")]
    UnsupportedDelimiter,
    #[error("native skill invocation flag is not a boolean")]
    InvalidInvocationFlag,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InvocationFlag {
    Boolean(bool),
    Text(String),
    Integer(i64),
}

impl InvocationFlag {
    fn into_boolean(self) -> Result<bool, SkillFileReadError> {
        match self {
            Self::Boolean(value) => Ok(value),
            Self::Text(value) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Ok(true),
                "0" | "false" | "no" | "off" => Ok(false),
                _ => Err(SkillFileReadError::InvalidInvocationFlag),
            },
            Self::Integer(1) => Ok(true),
            Self::Integer(0) => Ok(false),
            Self::Integer(_) => Err(SkillFileReadError::InvalidInvocationFlag),
        }
    }
}

#[derive(Default, Deserialize)]
struct SkillFrontmatterWire {
    version: Option<String>,
    hooks: Option<serde_json::Value>,
    #[serde(rename = "PreToolUse")]
    misplaced_pre_tool: Option<serde_json::Value>,
    #[serde(rename = "PermissionRequest")]
    misplaced_permission: Option<serde_json::Value>,
    paths: Option<serde_json::Value>,
    fallback: Option<InvocationFlag>,
    name: Option<String>,
    description: Option<String>,
    #[serde(
        rename = "user-invocable",
        default,
        deserialize_with = "read_present_flag"
    )]
    user_invocable: Option<InvocationFlag>,
    #[serde(
        rename = "disable-model-invocation",
        default,
        deserialize_with = "read_present_flag"
    )]
    disable_model_invocation: Option<InvocationFlag>,
}

fn read_present_flag<'de, D>(deserializer: D) -> Result<Option<InvocationFlag>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // A present null must not become an absent flag and gain a native default.
    InvocationFlag::deserialize(deserializer).map(Some)
}

pub fn read_claude_skill_metadata(
    contents: &str,
) -> Result<ClaudeSkillFileMetadata, SkillFileReadError> {
    let document =
        split_skill_document(contents).map_err(|_| SkillFileReadError::UnsupportedDelimiter)?;
    let Some(frontmatter) = document.frontmatter else {
        return Ok(ClaudeSkillFileMetadata::default());
    };
    // Reject tags whose semantics this reader does not implement. Includes and
    // property expansion are disabled at the dependency feature boundary too.
    let options = serde_saphyr::options! { reject_unsupported_tags: true };
    let wire: SkillFrontmatterWire = serde_saphyr::from_str_with_options(frontmatter, options)
        .map_err(|_| SkillFileReadError::InvalidYaml)?;
    Ok(ClaudeSkillFileMetadata {
        display_version: wire.version,
        has_hook_declarations: wire.hooks.is_some()
            || wire.misplaced_pre_tool.is_some()
            || wire.misplaced_permission.is_some(),
        has_conditional_paths: wire.paths.is_some(),
        fallback: wire
            .fallback
            .map(InvocationFlag::into_boolean)
            .transpose()?,
        display_name: wire.name,
        description: wire.description,
        user_invocable: wire
            .user_invocable
            .map(InvocationFlag::into_boolean)
            .transpose()?,
        disable_model_invocation: wire
            .disable_model_invocation
            .map(InvocationFlag::into_boolean)
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capabilities_frontmatter_preserves_quoted_block_and_alias_values() {
        // Oracle: YAML scalar/alias meanings; no native loader is replaced here.
        let metadata = read_claude_skill_metadata("---\nname: 'native:selection'\ndescription: |\n  Dollar $VALUE and café\n  second line\nuser-invocable: &enabled true\ndisable-model-invocation: *enabled\n---\nDo native work.\n").expect("valid native metadata");
        assert_eq!(metadata.display_name.as_deref(), Some("native:selection"));
        assert_eq!(
            metadata.description.as_deref(),
            Some("Dollar $VALUE and café\nsecond line\n")
        );
        assert_eq!(metadata.user_invocable, Some(true));
        assert_eq!(metadata.disable_model_invocation, Some(true));
    }

    #[test]
    fn native_capabilities_frontmatter_reads_crlf_bom_and_quoted_flags() {
        let metadata = read_claude_skill_metadata("\u{feff}---\r\nname: \"quoted skill\"\r\ndescription: >-\r\n  folded\r\n  description\r\nuser-invocable: 'false'\r\n---\r\nbody").expect("CRLF metadata");
        assert_eq!(metadata.display_name.as_deref(), Some("quoted skill"));
        assert_eq!(metadata.description.as_deref(), Some("folded description"));
        assert_eq!(metadata.user_invocable, Some(false));
        assert_eq!(metadata.disable_model_invocation, None);
    }

    #[test]
    fn native_capabilities_frontmatter_does_not_parse_or_evaluate_instruction_body() {
        let metadata = read_claude_skill_metadata(
            "---\nname: native\n---\n!include /not-a-header\nname: replacement\n",
        )
        .expect("metadata only");
        assert_eq!(metadata.display_name.as_deref(), Some("native"));
        assert_eq!(metadata.description, None);
        assert_eq!(
            read_claude_skill_metadata("ordinary instruction body").expect("no header"),
            ClaudeSkillFileMetadata::default()
        );
    }

    #[test]
    fn native_capabilities_frontmatter_rejects_invalid_unknown_tag_and_duplicate_keys() {
        for header in [
            "name: [unfinished",
            "name: !include /not-readable",
            "name: one\nname: two",
            "name: [not, scalar]",
            "user-invocable: null",
        ] {
            assert_eq!(
                read_claude_skill_metadata(&format!("---\n{header}\n---\nbody")),
                Err(SkillFileReadError::InvalidYaml),
                "{header}"
            );
        }
        assert_eq!(
            read_claude_skill_metadata("---\nuser-invocable: maybe\n---\n"),
            Err(SkillFileReadError::InvalidInvocationFlag)
        );
    }

    #[test]
    fn native_capabilities_frontmatter_uses_native_boolean_spellings_without_defaulting_unknown() {
        // Claude2.1.310 Cfn/mce and Ne/as accept these exact flag spellings.
        for (literal, expected) in [
            ("yes", true),
            ("on", true),
            ("1", true),
            ("no", false),
            ("off", false),
            ("0", false),
        ] {
            let metadata =
                read_claude_skill_metadata(&format!("---\nuser-invocable: {literal}\n---\n"))
                    .expect("native boolean spelling");
            assert_eq!(metadata.user_invocable, Some(expected));
        }
    }

    #[test]
    fn native_capabilities_frontmatter_never_silently_discards_an_unclosed_header() {
        assert_eq!(
            read_claude_skill_metadata("---\nname: missing-fence\n"),
            Err(SkillFileReadError::UnsupportedDelimiter)
        );
    }
}
