-- Rust's wake_slot_upgrade runs under the same outer transaction before this
-- index is installed. Session identity is independent of JSON key ordering.
CREATE UNIQUE INDEX wakeup_occupying_target
ON wakeup_definitions(
 json_extract(definition_json,'$.message.target.endpoint.serviceId'),
 json_extract(definition_json,'$.message.target.endpoint.endpointId'),
 json_extract(definition_json,'$.message.target.sessionId')
)
WHERE wakeup_status IN ('active','paused');
