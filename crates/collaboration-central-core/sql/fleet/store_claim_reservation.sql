INSERT INTO claim_reservations(attempt_id,project_id,state) VALUES(?,?,?) ON CONFLICT(attempt_id) DO UPDATE SET state=excluded.state;
