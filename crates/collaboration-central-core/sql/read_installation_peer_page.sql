SELECT peer_id, service_id FROM machine_enrollments WHERE (? IS NULL OR peer_id > ?) ORDER BY peer_id LIMIT ?;
