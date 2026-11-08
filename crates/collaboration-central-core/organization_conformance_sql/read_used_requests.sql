SELECT CAST(id AS TEXT) AS id,
    CAST(operation_id AS TEXT) AS operation_id,
    CAST(actor_person_id AS TEXT) AS actor_person_id,
    CAST(actor_session_id AS TEXT) AS actor_session_id,
    CAST(request_time_ms AS TEXT) AS request_time_ms,
    CAST(intent_digest AS TEXT) AS intent_digest
FROM organization_used_requests WHERE organization_id = ? ORDER BY id;
