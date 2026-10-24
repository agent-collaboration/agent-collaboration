//! Split a qualified SKILL.md header without interpreting its instruction body.
pub(super) struct SkillDocument<'a> {
    pub frontmatter: Option<&'a str>,
    pub body: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct UnsupportedSkillDelimiter;

pub(super) fn split_skill_document(
    contents: &str,
) -> Result<SkillDocument<'_>, UnsupportedSkillDelimiter> {
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
    let mut lines = contents.split_inclusive('\n');
    let Some(opening) = lines.next() else {
        return Ok(SkillDocument {
            frontmatter: None,
            body: contents,
        });
    };
    if opening.trim_end_matches(['\r', '\n', ' ', '\t']) != "---" {
        return Ok(SkillDocument {
            frontmatter: None,
            body: contents,
        });
    }
    let start = opening.len();
    let mut end = start;
    for line in lines {
        if line.trim_end_matches(['\r', '\n', ' ', '\t']) == "---" {
            let frontmatter = contents.get(start..end).ok_or(UnsupportedSkillDelimiter)?;
            let body = contents
                .get(end + line.len()..)
                .ok_or(UnsupportedSkillDelimiter)?;
            return Ok(SkillDocument {
                frontmatter: Some(frontmatter),
                body,
            });
        }
        end += line.len();
    }
    Err(UnsupportedSkillDelimiter)
}
