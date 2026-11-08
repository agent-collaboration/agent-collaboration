INSERT INTO organization_change_records(
    id, request_id, kind, origin, actor_person_id, permission_checked, target_id,
    created_at, updated_at
) VALUES(?, ?, ?, ?, ?, ?, ?, CAST(? AS INTEGER), CAST(? AS INTEGER));
