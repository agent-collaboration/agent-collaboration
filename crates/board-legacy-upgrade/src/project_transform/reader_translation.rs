//! Read progress is translated with the shipped main/reply distinction.
use super::{
    BTreeMap, BatchTiming, EndReason, FeedCursor, Identity, ImportWatch, MembershipEnd,
    MembershipState, ProjectBuilder, ReadMark, ReadScope, Record, RecordId, RecordPlacement,
    RecordedActor, SubscriptionLifetime, SubscriptionMembership, SubscriptionMembershipProps,
    SubscriptionPolicy, SubscriptionScope, ThreadId, UpgradeError, cursor, invalid,
};

impl ProjectBuilder<'_> {
    pub(super) fn finish_readers(
        &mut self,
    ) -> Result<(Vec<ReadMark>, Vec<ImportWatch>), UpgradeError> {
        let mut marks: BTreeMap<(Identity, String), FeedCursor> = BTreeMap::new();
        for row in &self.source.tracking {
            if row.project_id == self.scope.workspace
                && let Some(start) = row.main_start
            {
                marks.insert(
                    (
                        self.actor(&row.reader_key)?,
                        self.project.topic_id.as_str().to_owned(),
                    ),
                    self.cutoff(start)?,
                );
            }
        }
        for row in &self.source.project_marks {
            if row.scope_id == self.project.topic_id.as_str() {
                let key = (self.actor(&row.reader_key)?, row.scope_id.clone());
                let through = self.cutoff(row.through_activity)?;
                marks
                    .entry(key)
                    .and_modify(|old| {
                        if through.position > old.position {
                            *old = through
                        }
                    })
                    .or_insert(through);
            }
        }
        for row in &self.source.thread_marks {
            if self.belongs_to_thread(&row.scope_id) {
                marks.insert(
                    (self.actor(&row.reader_key)?, row.scope_id.clone()),
                    self.cutoff(row.through_activity)?,
                );
            }
        }
        let mut read_marks = Vec::new();
        for ((reader, scope_id), through) in marks {
            let scope = if scope_id == self.project.topic_id.as_str() {
                ReadScope::Project {
                    project_id: self.project.topic_id.clone(),
                }
            } else {
                ReadScope::Thread {
                    thread_id: ThreadId::try_from(scope_id.clone())
                        .map_err(|error| invalid("scope_id", error))?,
                }
            };
            let identity = serde_json::to_string(&RecordedActor::declared(reader.clone()))?;
            let id = RecordId::try_from(self.keys.derive("readMark", &[&identity, &scope_id])?)
                .map_err(|error| invalid("mark_id", error))?;
            let mut envelope = self.envelope(id, RecordedActor::declared(reader.clone()))?;
            envelope.position = RecordPlacement::Unsequenced;
            read_marks.push(ReadMark {
                envelope,
                reader,
                scope,
                through,
            });
        }
        let mut watches = Vec::new();
        for row in &self.source.project_watches {
            if row.scope_id == self.project.topic_id.as_str() {
                watches.push(ImportWatch {
                    reader: self.actor(&row.reader_key)?,
                    thread: None,
                    starts_after: self.cutoff(row.starts_after_activity)?,
                    active: row.active,
                });
            }
        }
        for row in &self.source.thread_watches {
            if self.belongs_to_thread(&row.scope_id) {
                watches.push(ImportWatch {
                    reader: self.actor(&row.reader_key)?,
                    thread: Some(
                        ThreadId::try_from(row.scope_id.clone())
                            .map_err(|error| invalid("scope_id", error))?,
                    ),
                    starts_after: self.cutoff(row.starts_after_activity)?,
                    active: row.active,
                });
            }
        }
        // Membership history has policy times but no activity position. Append its
        // retained facts in canonical source-key order after legacy activity.
        for row in &self.source.subscriptions {
            let scope = match row.scope_kind.as_str() {
                "topic" if row.scope_id == self.project.topic_id.as_str() => {
                    SubscriptionScope::project(self.project.topic_id.clone())
                }
                "topic" => continue,
                "thread" if self.belongs_to_thread(&row.scope_id) => SubscriptionScope::thread(
                    ThreadId::try_from(row.scope_id.clone())
                        .map_err(|error| invalid("scope_id", error))?,
                ),
                "thread" => continue,
                _ => return Err(invalid("scope_kind", "unknown subscription scope")),
            };
            let reader = self.actor(&row.reader_key)?;
            let at = self.next_cursor()?;
            let ended = match (row.state, row.end_reason, row.ended_at) {
                (MembershipState::Active, None, None) => None,
                (MembershipState::Draining, None, None) => Some(MembershipEnd {
                    reason: EndReason::Resolved,
                    position: at,
                    at: self.source.last_recorded_at.ok_or_else(|| {
                        invalid("draining.ended_at", "store has no recorded event time")
                    })?,
                }),
                (MembershipState::Ended, Some(reason), Some(ended_at)) => Some(MembershipEnd {
                    reason,
                    position: at,
                    at: ended_at,
                }),
                _ => return Err(invalid("membership", "ending fields disagree with state")),
            };
            let identity = serde_json::to_string(&RecordedActor::declared(reader.clone()))?;
            let generation = row.generation.get().to_string();
            let id = RecordId::try_from(self.keys.derive(
                "subscriptionMembership",
                &[&identity, &row.scope_kind, &row.scope_id, &generation],
            )?)
            .map_err(|error| invalid("membership_id", error))?;
            let record = SubscriptionMembership::new(SubscriptionMembershipProps {
                envelope: self.envelope(id, RecordedActor::declared(reader.clone()))?,
                reader: reader.clone(),
                scope,
                policy: SubscriptionPolicy::new(
                    &reader,
                    row.mode,
                    row.when_idle,
                    BatchTiming::new(row.quiet_seconds, row.cap_seconds)
                        .map_err(|error| invalid("timing", error))?,
                    SubscriptionLifetime::new(row.lifetime_seconds)
                        .map_err(|error| invalid("lifetime", error))?,
                )
                .map_err(|error| invalid("policy", error))?,
                renewed_at: row.renewed_at,
                expires_at: row.expires_at,
                generation: row.generation,
                activated_at: cursor(0)?,
                ended,
            })
            .map_err(|error| invalid("membership", error))?;
            self.push(Record::SubscriptionMembership(record), Vec::new())?;
        }
        Ok((read_marks, watches))
    }
}
