-- 2026-06-22: Rename sharing_settlement_payments to sharing_settlement_confirmations
--
-- Aligns the MySQL table with the renamed proto message
-- SettlementConfirmation. The column names also change:
--   paid_at  -> settled_at
--   created_by -> settled_by_participant_id
-- No data is lost: we ALTER TABLE RENAME which is a metadata-only
-- operation in MySQL 8 and preserves all rows.

ALTER TABLE sharing_settlement_payments
    RENAME TO sharing_settlement_confirmations;

-- The proto field "paid_at" becomes "settled_at". MySQL allows column
-- renames that keep the data intact.
ALTER TABLE sharing_settlement_confirmations
    CHANGE COLUMN paid_at settled_at DATE NOT NULL;

-- The proto field "created_by" becomes "settled_by_participant_id".
-- This column is unused by the current code (the field was added in
-- the proto but the migration hasn't tracked it). The repository
-- reads it via a SELECT *, so renaming keeps the data flow working.
ALTER TABLE sharing_settlement_confirmations
    CHANGE COLUMN from_user_id from_participant_id VARCHAR(64) NOT NULL,
    CHANGE COLUMN to_user_id to_participant_id VARCHAR(64) NOT NULL,
    CHANGE COLUMN created_by settled_by_participant_id VARCHAR(64) NOT NULL;
