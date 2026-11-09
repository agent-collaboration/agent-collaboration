SELECT subject FROM central_work_attempts WHERE kind=?1 AND state<>'given_up' AND next_attempt_at IS NOT NULL AND next_attempt_at<=CAST(?2 AS INTEGER) ORDER BY next_attempt_at, subject LIMIT 500;
