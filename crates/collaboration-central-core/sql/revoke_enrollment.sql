UPDATE machine_enrollments SET state = 'revoked', last_revocation_operation_id = ?, updated_at = ? WHERE machine_id = ?;
