UPDATE central_work_attempts SET next_attempt_at=CAST(?3 AS INTEGER),updated_at=CAST(?4 AS INTEGER) WHERE subject=?1 AND kind=?2 AND state='waiting' AND next_attempt_at IS NULL;
