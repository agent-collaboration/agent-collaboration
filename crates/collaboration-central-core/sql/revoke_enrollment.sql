UPDATE machine_enrollments SET state = 'revoked', last_revocation_operation = ? WHERE peer_id = ?;
