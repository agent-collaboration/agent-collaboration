SELECT CAST(MIN(next_attempt_at) AS TEXT) AS next_attempt_at, typeof(MIN(next_attempt_at)) AS next_attempt_class FROM central_work_attempts WHERE state<>'given_up' AND next_attempt_at IS NOT NULL;
