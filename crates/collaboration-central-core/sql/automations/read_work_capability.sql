SELECT service_id,protocols_json,CAST(observed_at_ms AS TEXT) AS observed_at_ms FROM machine_work_capabilities WHERE service_id=?;
