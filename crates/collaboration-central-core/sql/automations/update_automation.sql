UPDATE automations SET revision=?,definition_json=?,state=?,slot_session_key=?,updated_at_ms=CAST(? AS INTEGER),fault_json=NULL,faulted_at_ms=NULL WHERE automation_id=?;
