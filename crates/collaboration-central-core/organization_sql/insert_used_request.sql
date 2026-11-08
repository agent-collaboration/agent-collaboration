INSERT INTO organization_used_requests(
    id, organization_id, request_time_ms, operation_id, actor_person_id,
    actor_session_id, intent_digest, proof_key_thumbprint, created_at, updated_at
) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9);
