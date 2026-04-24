-- Expenses in a sharing budget
CREATE TABLE IF NOT EXISTS sharing_expenses (
    id           VARCHAR(36)  NOT NULL PRIMARY KEY,
    budget_id    VARCHAR(36)  NOT NULL,
    paid_by      VARCHAR(36)  NOT NULL,   -- user_id of who paid
    total_amount BIGINT       NOT NULL,
    description  VARCHAR(500) NOT NULL DEFAULT '',
    expense_date VARCHAR(20)  NOT NULL,
    category_id  VARCHAR(36)           DEFAULT NULL,
    split_method VARCHAR(20)  NOT NULL DEFAULT 'equal' COMMENT 'equal | custom | weighted',
    created_by   VARCHAR(36)  NOT NULL,
    created_at   BIGINT       NOT NULL,
    updated_at   BIGINT       NOT NULL,
    deleted_at   BIGINT                DEFAULT NULL,
    INDEX idx_sharing_expenses_budget (budget_id),
    INDEX idx_sharing_expenses_paid_by (paid_by)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Per-participant legs for each expense
CREATE TABLE IF NOT EXISTS sharing_expense_legs (
    id         VARCHAR(36) NOT NULL PRIMARY KEY,
    expense_id VARCHAR(36) NOT NULL,
    user_id    VARCHAR(36) NOT NULL,
    amount     BIGINT      NOT NULL,
    created_at BIGINT      NOT NULL,
    INDEX idx_sharing_legs_expense (expense_id),
    INDEX idx_sharing_legs_user    (user_id),
    FOREIGN KEY (expense_id) REFERENCES sharing_expenses(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Running net balances per user per budget (positive = owed to user, negative = user owes)
CREATE TABLE IF NOT EXISTS sharing_balances (
    id          VARCHAR(36) NOT NULL PRIMARY KEY,
    budget_id   VARCHAR(36) NOT NULL,
    user_id     VARCHAR(36) NOT NULL,
    net_balance BIGINT      NOT NULL DEFAULT 0,
    updated_at  BIGINT      NOT NULL,
    UNIQUE KEY uk_sharing_balance (budget_id, user_id),
    INDEX idx_sharing_balances_budget (budget_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Invite-by-link tokens for sharing budgets
CREATE TABLE IF NOT EXISTS sharing_join_links (
    id         VARCHAR(36)  NOT NULL PRIMARY KEY,
    budget_id  VARCHAR(36)  NOT NULL,
    token      VARCHAR(64)  NOT NULL UNIQUE,
    created_by VARCHAR(36)  NOT NULL,
    created_at BIGINT       NOT NULL,
    expires_at BIGINT       NOT NULL,
    INDEX idx_sharing_join_links_token (token),
    INDEX idx_sharing_join_links_budget (budget_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
