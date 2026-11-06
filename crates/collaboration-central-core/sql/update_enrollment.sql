UPDATE machine_enrollments SET machine_label = ?, access_client_id = ?, credential_hash = ?, state = 'enrolled', expires_at = ?, updated_at = ? WHERE machine_id = ?;
