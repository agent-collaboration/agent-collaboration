CREATE INDEX trail_current_position ON record_identities(json_extract(payload, '$.record.envelope.position.position.position') DESC, id DESC) WHERE kind = 'workTrailRow';
CREATE INDEX move_outcome_position ON record_identities(json_extract(payload, '$.record.outcome.kind'), json_extract(payload, '$.record.envelope.position.position.position') DESC, id DESC) WHERE kind = 'move';
