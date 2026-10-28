//! Decision 1049: retired global activity intervals become imported project cursors.
use crate::{
    MessageUpgradeError, UpgradeError, import_error,
    legacy_rows::{ActivityKind, LegacyProject, LegacyStore},
    message_rows::{MessageRows, invalid},
};
use collaboration_protocol::{MAX_PUSH_ACTIVITY_RANGES, PushActivityRange, PushActivitySnapshot};
use message_board::{ActivitySequence, FeedCursor, PostId, ProjectId, ThreadId};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Default)]
pub struct MessageActivityMap {
    threads: BTreeMap<ThreadId, ImportedThreadActivity>,
    projects: BTreeSet<ProjectId>,
}

#[derive(Clone)]
struct ImportedThreadActivity {
    project: ProjectId,
    project_mapping: Arc<BTreeMap<u64, FeedCursor>>,
    activity: BTreeSet<u64>,
}

// The retired protocol no longer owns these shapes. Keep its strict wire decoder
// here, at the one-time migration boundary, rather than accepting both shapes live.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyActivitySnapshot {
    ranges: Vec<LegacyActivityRange>,
    held: bool,
    draining: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyActivityRange {
    root_message_id: PostId,
    from_activity_sequence: ActivitySequence,
    through_activity_sequence: ActivitySequence,
}

impl MessageActivityMap {
    pub(crate) fn insert_project(
        &mut self,
        source: &LegacyStore,
        project: &LegacyProject,
        mapping: &BTreeMap<u64, FeedCursor>,
    ) -> Result<(), UpgradeError> {
        self.projects.insert(project.topic_id.clone());
        let project_mapping = Arc::new(mapping.clone());
        for post in source
            .posts
            .values()
            .filter(|post| post.topic_id == project.topic_id && post.root_id.is_none())
        {
            let thread = ThreadId::from(post.message_id.clone());
            let activity = source
                .activity
                .iter()
                .filter(|event| {
                    event.topic_id == project.topic_id
                        && (event.root_id.as_ref() == Some(&thread)
                            || (matches!(event.kind, ActivityKind::MainMessageCreated)
                                && event.message_id.as_ref() == Some(&post.message_id)))
                })
                .map(|event| event.activity_sequence)
                .collect();
            if self.threads.contains_key(&thread) {
                return Err(import_error::invalid(
                    "push.thread_mapping",
                    "duplicate imported root",
                ));
            }
            self.threads.insert(
                thread,
                ImportedThreadActivity {
                    project: project.topic_id.clone(),
                    project_mapping: project_mapping.clone(),
                    activity,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn imported_thread(&self, root: PostId) -> Option<ThreadId> {
        let thread = ThreadId::from(root);
        self.threads.contains_key(&thread).then_some(thread)
    }

    pub(crate) fn imported_project(&self, topic: ProjectId) -> Option<ProjectId> {
        self.projects.contains(&topic).then_some(topic)
    }

    pub(crate) fn translate(&self, rows: &mut MessageRows) -> Result<(), MessageUpgradeError> {
        let mut missing_thread_ranges = 0;
        let mut empty_thread_ranges = 0;
        for push in &mut rows.pushes {
            let Some(encoded) = &push.ranges_json else {
                continue;
            };
            let legacy: LegacyActivitySnapshot =
                serde_json::from_str(encoded).map_err(|_| invalid("push.legacy_ranges"))?;
            if legacy.ranges.is_empty() || legacy.ranges.len() > MAX_PUSH_ACTIVITY_RANGES {
                return Err(invalid("push.legacy_ranges"));
            }
            let mut roots = BTreeSet::new();
            let mut ranges = Vec::new();
            for range in legacy.ranges {
                let from = range.from_activity_sequence.get();
                let through = range.through_activity_sequence.get();
                if from > through || !roots.insert(range.root_message_id.clone()) {
                    return Err(invalid("push.legacy_ranges"));
                }
                let thread_id = ThreadId::from(range.root_message_id);
                let Some(thread) = self.threads.get(&thread_id) else {
                    missing_thread_ranges += 1;
                    continue;
                };
                if thread.activity.range(from..=through).next().is_none() {
                    empty_thread_ranges += 1;
                    continue;
                }
                // Ceiling/floor are restricted to this project's real imported rows;
                // a global bound may have named an activity in a different project.
                let mut covered = thread.project_mapping.range(from..=through);
                let (_, first) = covered
                    .next()
                    .ok_or_else(|| invalid("push.activity_mapping"))?;
                let last = covered.next_back().map_or(first, |(_, cursor)| cursor);
                ranges.push(PushActivityRange {
                    thread_id,
                    project_id: thread.project.clone(),
                    from: *first,
                    through: *last,
                });
            }
            push.ranges_json = Some(serde_json::to_string(&PushActivitySnapshot {
                ranges,
                held: legacy.held,
                draining: legacy.draining,
            })?);
        }
        if missing_thread_ranges != 0 || empty_thread_ranges != 0 {
            return Err(MessageUpgradeError::UnmappedActivityRanges {
                missing_thread_ranges,
                empty_thread_ranges,
            });
        }
        for push in &rows.pushes {
            push.validate()?;
        }
        Ok(())
    }
}
