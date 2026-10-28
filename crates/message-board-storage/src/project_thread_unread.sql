WITH subject AS (SELECT 'thread' AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND CASE record_kind
WHEN 'thread' THEN record_id = subject.id
WHEN 'threadRefiled' THEN EXISTS (SELECT 1 FROM thread_refilings AS refile WHERE refile.refiling_id = project_feed.record_id AND refile.thread_id = subject.id)
WHEN 'post' THEN record_id = subject.id OR EXISTS (SELECT 1 FROM posts AS post WHERE post.post_id = project_feed.record_id AND post.thread_id = subject.id)
WHEN 'fact' THEN
 EXISTS (SELECT 1 FROM json_each(json_extract(content, '$.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR CASE json_extract(content, '$.record.record.kind.kind')
 WHEN 'evaluatorResult' THEN json_extract(content, '$.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(content, '$.record.record.kind.fact.subject.id') = subject.id
 WHEN 'lifecycle' THEN json_extract(content, '$.record.record.kind.fact.kind') = 'threadResolved' AND json_extract(content, '$.record.record.kind.fact.thread') = subject.id
 ELSE 0 END
WHEN 'notifyIntent' THEN EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id)
WHEN 'request' THEN json_extract(content, '$.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM project_feed AS effect INDEXED BY feed_by_request_position WHERE effect.request_id = project_feed.request_id AND effect.record_kind = subject.kind AND effect.record_id = subject.id AND effect.record_kind <> 'request')
ELSE 0 END
ORDER BY position DESC LIMIT 50
