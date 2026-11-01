UPDATE automation_firings SET state='received' WHERE batch_id=? AND state IN ('pending','held') AND report_seq=0;
