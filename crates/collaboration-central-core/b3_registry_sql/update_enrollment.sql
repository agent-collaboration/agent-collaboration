UPDATE machine_enrollments
SET state = ?, end_reason = ?, machine_client_id = ?, last_grant_at = CAST(? AS INTEGER),
    ended_at = CAST(? AS INTEGER), updated_at = CAST(? AS INTEGER)
WHERE id = ?;
