use crate::sealed_source::{LegacyActivityKind, LegacySourceRows};
use serde::{Serialize, Serializer, ser::SerializeStruct};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SelectionMode {
    Latest,
    Unread,
}

impl SelectionMode {
    pub(crate) const ORDERED: [Self; 2] = [Self::Latest, Self::Unread];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct MainStartCutoff {
    pub(crate) legacy_workspace: String,
    pub(crate) main_start: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ThreadCutoff {
    pub(crate) root_id: Option<String>,
    pub(crate) watch: ThreadWatchCutoff,
    pub(crate) thread_mark_through_activity: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ThreadWatchCutoff {
    Absent,
    Present {
        active: bool,
        starts_after_activity: i64,
    },
}

impl Serialize for ThreadCutoff {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ThreadCutoff", 4)?;
        state.serialize_field("root_id", &self.root_id)?;
        match &self.watch {
            ThreadWatchCutoff::Absent => {
                state.serialize_field("watch_active", &Option::<bool>::None)?;
                state.serialize_field("watch_starts_after_activity", &Option::<i64>::None)?;
            }
            ThreadWatchCutoff::Present {
                active,
                starts_after_activity,
            } => {
                state.serialize_field("watch_active", active)?;
                state.serialize_field("watch_starts_after_activity", starts_after_activity)?;
            }
        }
        state.serialize_field(
            "thread_mark_through_activity",
            &self.thread_mark_through_activity,
        )?;
        state.end()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ReaderCutoffs {
    pub(crate) reader: String,
    pub(crate) main_starts: Vec<MainStartCutoff>,
    pub(crate) topic_mark_through_activity: i64,
    pub(crate) thread_cutoffs: Vec<ThreadCutoff>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OraclePostSelection {
    pub(crate) reader: String,
    pub(crate) legacy_topic: String,
    pub(crate) mode: SelectionMode,
    pub(crate) post_ids: Vec<String>,
    pub(crate) post_count: u64,
    pub(crate) covered_post_set_sha256: String,
}

pub(crate) fn selected_post_ids(
    source: &LegacySourceRows,
    reader: &str,
    topic: &str,
    mode: SelectionMode,
) -> Vec<String> {
    let mut selected = source
        .activity
        .iter()
        .filter_map(|activity| {
            if activity.topic_id != topic || activity.message_id.is_none() {
                return None;
            }
            if mode == SelectionMode::Unread && activity.actor_key == reader {
                return None;
            }

            let message_id = activity.message_id.as_ref()?;
            let include = match activity.kind {
                LegacyActivityKind::MainMessageCreated => {
                    if mode == SelectionMode::Latest {
                        true
                    } else {
                        let main_start = source
                            .main_starts
                            .iter()
                            .find(|row| {
                                row.reader_key == reader && row.project_id == activity.project_id
                            })
                            .and_then(|row| row.main_start);
                        let topic_mark = source
                            .topic_marks
                            .iter()
                            .find(|row| row.reader_key == reader && row.topic_id == topic)
                            .map_or(0, |row| row.through_activity);
                        main_start
                            .is_some_and(|start| activity.activity_sequence > start.max(topic_mark))
                    }
                }
                LegacyActivityKind::ThreadMessageCreated => {
                    let watch = source.thread_watches.iter().find(|row| {
                        row.reader_key == reader
                            && Some(row.root_id.as_str()) == activity.root_id.as_deref()
                    });
                    watch.is_some_and(|watch| {
                        watch.active
                            && (mode == SelectionMode::Latest
                                || activity.activity_sequence
                                    > watch.starts_after_activity.max(
                                        source
                                            .thread_marks
                                            .iter()
                                            .find(|row| {
                                                row.reader_key == reader
                                                    && Some(row.root_id.as_str())
                                                        == activity.root_id.as_deref()
                                            })
                                            .map_or(0, |row| row.through_activity),
                                    ))
                    })
                }
                LegacyActivityKind::Other => false,
            };

            include.then(|| (activity.activity_sequence, message_id.clone()))
        })
        .collect::<Vec<_>>();

    selected.sort_by(|left, right| {
        let ascending_order = left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1));
        if mode == SelectionMode::Latest {
            ascending_order.reverse()
        } else {
            ascending_order
        }
    });

    selected
        .into_iter()
        .map(|(_, message_id)| message_id)
        .collect()
}

pub(crate) fn derive_reader_cutoffs(
    source: &LegacySourceRows,
    reader: &str,
    topic: &str,
) -> ReaderCutoffs {
    let legacy_workspaces = source
        .activity
        .iter()
        .filter(|activity| {
            activity.topic_id == topic
                && activity.message_id.is_some()
                && activity.kind == LegacyActivityKind::MainMessageCreated
        })
        .map(|activity| activity.project_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let main_starts = legacy_workspaces
        .into_iter()
        .map(|legacy_workspace| MainStartCutoff {
            main_start: source
                .main_starts
                .iter()
                .find(|row| row.reader_key == reader && row.project_id == legacy_workspace)
                .and_then(|row| row.main_start),
            legacy_workspace,
        })
        .collect();
    let topic_mark_through_activity = source
        .topic_marks
        .iter()
        .find(|row| row.reader_key == reader && row.topic_id == topic)
        .map_or(0, |row| row.through_activity);
    let thread_roots = source
        .activity
        .iter()
        .filter(|activity| {
            activity.topic_id == topic
                && activity.message_id.is_some()
                && activity.kind == LegacyActivityKind::ThreadMessageCreated
        })
        .map(|activity| activity.root_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let thread_cutoffs = thread_roots
        .into_iter()
        .map(|root_id| {
            let watch = source.thread_watches.iter().find(|row| {
                row.reader_key == reader && Some(row.root_id.as_str()) == root_id.as_deref()
            });
            ThreadCutoff {
                thread_mark_through_activity: source
                    .thread_marks
                    .iter()
                    .find(|row| {
                        row.reader_key == reader && Some(row.root_id.as_str()) == root_id.as_deref()
                    })
                    .map_or(0, |row| row.through_activity),
                watch: watch.map_or(ThreadWatchCutoff::Absent, |row| {
                    ThreadWatchCutoff::Present {
                        active: row.active,
                        starts_after_activity: row.starts_after_activity,
                    }
                }),
                root_id,
            }
        })
        .collect();

    ReaderCutoffs {
        reader: reader.to_owned(),
        main_starts,
        topic_mark_through_activity,
        thread_cutoffs,
    }
}
