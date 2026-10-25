-- The owning migrator gates this step until the stopped-Router upgrade has
-- verified the copy. Pre-release mail_items must always be empty.
DROP TABLE mail_items;
DROP TABLE router_pushes;
DROP TABLE subscription_delivery_progress;
DROP TABLE board_intent_progress;
DROP TABLE board_intent_scan_cursors;
