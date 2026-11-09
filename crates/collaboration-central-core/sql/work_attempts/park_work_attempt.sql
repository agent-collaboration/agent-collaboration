UPDATE central_work_attempts SET next_attempt_at=NULL,updated_at=CAST(?3 AS INTEGER) WHERE subject=?1 AND kind=?2 AND state='waiting';
