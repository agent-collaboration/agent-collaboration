UPDATE automation_timing SET next_due_ms=CAST(? AS INTEGER),expires_at_ms=CAST(? AS INTEGER) WHERE automation_id=?;
