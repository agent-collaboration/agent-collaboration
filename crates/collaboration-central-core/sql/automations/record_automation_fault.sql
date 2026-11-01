UPDATE automations SET fault_json=?,faulted_at_ms=CAST(? AS INTEGER) WHERE automation_id=?;
