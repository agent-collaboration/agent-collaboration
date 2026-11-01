-- The Fleet owns central definition slots. Local rows retain the legacy slot invariant.
DROP INDEX wakeup_occupying_target;
CREATE UNIQUE INDEX wakeup_occupying_target
ON wakeup_definitions(
 json_extract(definition_json,'$.message.target.endpoint.serviceId'),
 json_extract(definition_json,'$.message.target.endpoint.endpointId'),
 json_extract(definition_json,'$.message.target.sessionId')
)
WHERE authority = 'local' AND wakeup_status IN ('active','paused');
