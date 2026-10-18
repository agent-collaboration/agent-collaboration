DROP INDEX one_intent_replacement;
CREATE INDEX intents_by_cause ON notify_intents(cause_kind, cause_id, intent_id);
CREATE UNIQUE INDEX one_intent_per_actor ON notify_intents(cause_kind, cause_id, json_extract(recipient, '$.recipient.actor.actor.identity')) WHERE json_extract(recipient, '$.kind') = 'resolved';
CREATE UNIQUE INDEX one_empty_role_per_cause ON notify_intents(cause_kind, cause_id, json_extract(recipient, '$.recipient.subject.kind'), json_extract(recipient, '$.recipient.subject.subject'), json_extract(recipient, '$.recipient.role')) WHERE json_extract(recipient, '$.kind') = 'noHolder';
