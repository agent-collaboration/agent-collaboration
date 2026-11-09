SELECT CAST(count(*) AS TEXT) AS in_flight FROM central_work_attempts WHERE connection_id=?1 AND state='in_flight';
