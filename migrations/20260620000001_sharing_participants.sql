-- 2026-06-20: Add sharing_participants table for the guest identity model
--
-- Guests are account-free participants: they join a sharing budget by entering
-- a display name and receive an opaque session token (SHA-256 hash stored).
-- Members are Normal Users who already hold a Budget role on the parent
-- budget; their user_id is the same as the identity-service user_id.
--
-- Both kinds share the same column space (user_id, display_name) so that
-- sharing_expense_legs.user_id and sharing_balances.user_id can reference
-- either without a join. The convention is:
--   member: user_id = "u_<identity-user-id>"  (or just the bare id)
--   guest:  user_id = "g_<random-uuid>"
--
-- session_token_hash is only set for guests; always NULL for members
-- (members authenticate via the Identity service JWT, not via this table).
-- We never store the raw token, only its SHA-256 hex digest.

CREATE TABLE IF NOT EXISTS sharing_participants (
    id                  CHAR(36)      NOT NULL PRIMARY KEY,
    budget_id           CHAR(36)      NOT NULL,
    participant_kind    VARCHAR(16)   NOT NULL DEFAULT 'guest',  -- 'guest' | 'member'
    user_id             VARCHAR(64)   DEFAULT NULL,             -- bare id; prefixed 'g_' for guests
    display_name        VARCHAR(120)  NOT NULL,
    session_token_hash  CHAR(64)      DEFAULT NULL,             -- sha256 hex; only for guests
    joined_at           BIGINT        NOT NULL,
    last_seen_at        BIGINT        NOT NULL,
    revoked_at          BIGINT        DEFAULT NULL,

    -- A budget has at most one member row per identity user.
    -- (Guests can have multiple historical rows for re-join support;
    --  the application enforces case-insensitive display-name uniqueness
    --  among active guests.)
    UNIQUE KEY uk_sharing_participant_member (budget_id, user_id),
    INDEX idx_sharing_participants_budget (budget_id),
    INDEX idx_sharing_participants_token (budget_id, session_token_hash)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
