-- Per-expense items for BY_ITEM split.
-- Created in Phase 2 P0 fix QA-005 — previously the proto's `items`
-- field was hardcoded to `vec![]` on every response and items were
-- never persisted, so BY_ITEM expenses had no audit trail of which
-- items existed or who shared them.
CREATE TABLE IF NOT EXISTS sharing_expense_items (
    id         VARCHAR(36) NOT NULL PRIMARY KEY,
    expense_id VARCHAR(36) NOT NULL,
    label      VARCHAR(200) NOT NULL,
    amount     BIGINT      NOT NULL,
    created_at BIGINT      NOT NULL,
    INDEX idx_sharing_items_expense (expense_id),
    FOREIGN KEY (expense_id) REFERENCES sharing_expenses(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Per-item assignments (which participant shares which item, in what
-- proportion). Numerator is the share of the item — denominator is
-- the sum of all numerators on that item.
CREATE TABLE IF NOT EXISTS sharing_expense_item_assignments (
    id         VARCHAR(36) NOT NULL PRIMARY KEY,
    item_id    VARCHAR(36) NOT NULL,
    user_id    VARCHAR(36) NOT NULL,
    numerator  INT         NOT NULL,
    INDEX idx_sharing_item_assignments_item (item_id),
    FOREIGN KEY (item_id) REFERENCES sharing_expense_items(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
