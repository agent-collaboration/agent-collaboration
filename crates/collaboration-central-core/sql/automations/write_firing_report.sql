UPDATE automation_firings SET report_seq=CAST(? AS INTEGER),state=?,outcome_json=? WHERE firing_id=?;
