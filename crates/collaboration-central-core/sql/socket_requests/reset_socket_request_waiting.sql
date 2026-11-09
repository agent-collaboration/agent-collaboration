UPDATE central_work_attempts SET state='waiting',send_id=NULL,connection_id=NULL,session_id=NULL,sent_version=NULL,next_attempt_at=NULL,updated_at=CAST(?3 AS INTEGER) WHERE subject=?1 AND kind=?2;
