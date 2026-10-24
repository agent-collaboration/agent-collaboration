UPDATE machine_notice_state SET pruned_through=CAST(? AS INTEGER) WHERE name='machine';
