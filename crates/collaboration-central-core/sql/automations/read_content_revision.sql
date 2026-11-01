SELECT content_id,owner_id,revision_id,CAST(recorded_at_ms AS TEXT) AS recorded_at_ms,digest,text FROM content_revisions WHERE content_id=?;
