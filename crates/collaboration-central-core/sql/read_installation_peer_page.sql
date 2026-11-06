SELECT machine_id AS peer_id, service_id FROM machine_enrollments WHERE (? IS NULL OR machine_id > ?) ORDER BY machine_id LIMIT ?;
