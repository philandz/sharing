-- Add weight column to support weighted split (Bead 0.2 / 2026-06-09 plan)
-- Backfills existing rows to 0 (legacy legs were equal/custom split).
ALTER TABLE sharing_expense_legs
  ADD COLUMN weight BIGINT NOT NULL DEFAULT 0;
