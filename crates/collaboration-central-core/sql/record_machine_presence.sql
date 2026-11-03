UPDATE machine_enrollments SET last_seen_at = ?1, updated_at = ?1 WHERE machine_id = ?2 AND state = 'enrolled';
