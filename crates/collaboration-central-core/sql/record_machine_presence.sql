UPDATE machine_enrollments SET last_seen = ? WHERE peer_id = ? AND state = 'enrolled';
