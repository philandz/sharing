-- 2026-06-23: Widen identifier columns to accommodate guest
-- participant_ids ("g_<36-char-uuid>" = 39 chars). Members use the
-- bare user_id which is at most 36 chars.

ALTER TABLE sharing_expenses        MODIFY COLUMN paid_by    VARCHAR(64) NOT NULL;
ALTER TABLE sharing_expenses        MODIFY COLUMN created_by VARCHAR(64) NOT NULL;
ALTER TABLE sharing_expense_legs    MODIFY COLUMN user_id    VARCHAR(64) NOT NULL;
ALTER TABLE sharing_balances        MODIFY COLUMN user_id    VARCHAR(64) NOT NULL;
ALTER TABLE sharing_settlement_confirmations MODIFY COLUMN from_participant_id VARCHAR(64) NOT NULL;
ALTER TABLE sharing_settlement_confirmations MODIFY COLUMN to_participant_id   VARCHAR(64) NOT NULL;
ALTER TABLE sharing_settlement_confirmations MODIFY COLUMN settled_by_participant_id VARCHAR(64) NOT NULL;
ALTER TABLE sharing_expense_comments MODIFY COLUMN author_participant_id      VARCHAR(64) NOT NULL;
ALTER TABLE sharing_activity_log     MODIFY COLUMN actor_participant_id       VARCHAR(64) NOT NULL;

