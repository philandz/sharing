-- Settlement payments: records that a member has paid another member
-- outside the app, or via the in-app "Mark as paid" flow.
-- (Bead 0.2 / Task 2.2)
CREATE TABLE IF NOT EXISTS sharing_settlement_payments (
    id           VARCHAR(36)  NOT NULL PRIMARY KEY,
    budget_id    VARCHAR(36)  NOT NULL,
    from_user_id VARCHAR(128) NOT NULL,
    to_user_id   VARCHAR(128) NOT NULL,
    amount       BIGINT       NOT NULL,
    paid_at      DATE         NOT NULL,
    note         TEXT         NULL,
    created_by   VARCHAR(128) NOT NULL,
    created_at   BIGINT       NOT NULL,
    INDEX idx_sharing_payments_budget (budget_id, paid_at),
    CONSTRAINT chk_settlement_payment_amount_positive CHECK (amount > 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
