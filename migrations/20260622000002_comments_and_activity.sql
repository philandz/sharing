-- 2026-06-22: Add per-expense comments and activity log tables.
--
-- sharing_expense_comments: flat per-expense comments (no threading).
-- author_participant_id is the bare id; can be a member's user_id or a
-- guest's g_<uuid>.
--
-- sharing_activity_log: append-only audit trail. actor_display_name is
-- denormalized at write time so log entries remain readable after the
-- participant leaves or is renamed.

CREATE TABLE IF NOT EXISTS sharing_expense_comments (
    id                      CHAR(36)     NOT NULL PRIMARY KEY,
    expense_id              CHAR(36)     NOT NULL,
    author_participant_id   VARCHAR(64)  NOT NULL,
    author_display_name     VARCHAR(120) NOT NULL,
    body                    TEXT         NOT NULL,
    created_at              BIGINT       NOT NULL,
    deleted_at              BIGINT       DEFAULT NULL,
    INDEX idx_sharing_comments_expense (expense_id, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS sharing_activity_log (
    id                   CHAR(36)     NOT NULL PRIMARY KEY,
    budget_id            CHAR(36)     NOT NULL,
    actor_participant_id VARCHAR(64)  NOT NULL,
    actor_display_name   VARCHAR(120) NOT NULL,
    action               VARCHAR(64)  NOT NULL,
    target_type          VARCHAR(32)  NOT NULL DEFAULT '',
    target_id            VARCHAR(64)  NOT NULL DEFAULT '',
    metadata_json        TEXT         NOT NULL DEFAULT '{}',
    created_at           BIGINT       NOT NULL,
    INDEX idx_sharing_activity_budget (budget_id, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
