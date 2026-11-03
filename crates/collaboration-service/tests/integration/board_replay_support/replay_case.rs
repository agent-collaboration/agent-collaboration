//! Source metadata stays beside the independent expected answers; no history is invented here.
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCase {
    pub case: String,
    pub summary: String,
    pub sources: Vec<String>,
    pub actors: Vec<ReplayActor>,
    pub setup: Vec<ReplaySetup>,
    pub events: Vec<ReplayEvent>,
    pub questions: Vec<ReplayQuestion>,
    pub probes: Vec<ReplayProbe>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayActor {
    pub alias: String,
    pub kind: ReplayActorKind,
    pub note: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayActorKind {
    Session,
    Human,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaySetup {
    pub op: String,
    pub args: Value,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayEvent {
    pub seq: usize,
    pub at: String,
    pub actor: String,
    pub acting_for: Option<String>,
    pub op: String,
    pub args: Value,
    pub note: String,
    pub source: Option<String>,
    pub approx_time: Option<bool>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayQuestion {
    pub ask: QuestionKind,
    pub via: String,
    pub as_of: Option<usize>,
    pub expect: Value,
}
#[derive(Clone, Deserialize)]
pub enum QuestionKind {
    #[serde(rename = "what was decided")]
    Decisions,
    #[serde(rename = "where did it stop")]
    State,
    #[serde(rename = "what proves it")]
    Evidence,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayProbe {
    pub after: usize,
    pub actor: String,
    pub op: String,
    pub args: Value,
    pub expect: Value,
    pub note: String,
}

impl ReplayCase {
    pub fn validate_metadata(&self) -> Result<(), super::board_fixture::TestError> {
        if self.case.is_empty()
            || self.summary.is_empty()
            || self.sources.is_empty()
            || self.sources.iter().any(String::is_empty)
            || self
                .actors
                .iter()
                .any(|actor| actor.alias.is_empty() || actor.note.is_empty())
        {
            return Err("incomplete replay provenance".into());
        }
        let mut previous = None;
        for (index, event) in self.events.iter().enumerate() {
            let time = event.at.parse::<chrono::DateTime<chrono::Utc>>()?;
            if event.seq != index + 1
                || previous.is_some_and(|before| time < before)
                || event.note.is_empty()
                || event.source.as_ref().is_some_and(String::is_empty)
            {
                return Err("invalid ordered replay source".into());
            }
            if event.approx_time == Some(true) && event.note.is_empty() {
                return Err("inferred event needs its annotation".into());
            }
            previous = Some(time);
        }
        if self
            .questions
            .iter()
            .any(|question| question.via.is_empty())
            || self.probes.iter().any(|probe| probe.note.is_empty())
        {
            return Err("missing replay observation/probe description".into());
        }
        Ok(())
    }
}
