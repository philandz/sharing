-- 2026-07-13: Store org_id in join links so guests can enrich participant
-- display names from identity without a separate budget-service call.
-- Guests receive the org_id embedded in their join token rather than
-- calling the budget service (which would require a member JWT they don't have).

ALTER TABLE sharing_join_links
  ADD COLUMN org_id VARCHAR(64) NULL AFTER budget_id;

ALTER TABLE sharing_participants
  ADD COLUMN org_id VARCHAR(64) NULL AFTER budget_id;

-- Backfill org_id for existing join links: the link creator is a member,
-- so we can look up the org_id via the budget service. Run manually:
--   UPDATE sharing_join_links j
--   JOIN budgets b ON b.id = j.budget_id
--   SET j.org_id = b.org_id
--   WHERE j.org_id IS NULL;
