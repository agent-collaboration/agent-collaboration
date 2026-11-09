INSERT INTO machine_enrollments(
    id, installation_id, request_id, state, end_reason, approved_by_person_id, permission_checked,
    base_generation, code_expires_at, machine_client_id, last_grant_at, ended_at, created_at, updated_at
) VALUES(?, ?, ?, ?, ?, ?, ?, CAST(? AS INTEGER), CAST(? AS INTEGER), ?, CAST(? AS INTEGER),
    CAST(? AS INTEGER), CAST(? AS INTEGER), CAST(? AS INTEGER));
