use anyhow::Result;
use philand_time::now_unix;
use sqlx::MySqlPool;

use crate::converters::{
    split_method_to_db, DbBalance, DbExpense, DbExpenseItem, DbExpenseItemAssignment, DbExpenseLeg,
    DbParticipant,
};
use crate::pb::service::sharing::{SettlementConfirmation, SplitMethod};

pub struct SharingRepository {
    pool: MySqlPool,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl SharingRepository {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = sqlx::MySqlPool::connect(database_url).await?;
        let mut migrator =
            sqlx::migrate::Migrator::new(std::path::Path::new("./migrations")).await?;
        migrator.set_ignore_missing(true);
        migrator.run(&pool).await?;
        Ok(Self { pool })
    }

    // -----------------------------------------------------------------------
    // Expenses
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn create_expense(
        &self,
        budget_id: &str,
        paid_by: &str,
        total_amount: i64,
        description: &str,
        expense_date: &str,
        category_id: Option<&str>,
        split_method: SplitMethod,
        legs: &[(String, i64, i64)], // (user_id, amount, weight)
        created_by: &str,
    ) -> Result<DbExpense> {
        let id = new_id();
        let now = now_unix();
        let method_str = split_method_to_db(split_method);

        sqlx::query(
            "INSERT INTO sharing_expenses (id, budget_id, paid_by, total_amount, description, expense_date, category_id, split_method, created_by, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id).bind(budget_id).bind(paid_by).bind(total_amount)
        .bind(description).bind(expense_date).bind(category_id)
        .bind(method_str).bind(created_by).bind(now).bind(now)
        .execute(&self.pool).await?;

        // Insert legs (amount and weight)
        for (user_id, amount, weight) in legs {
            let leg_id = new_id();
            sqlx::query(
                "INSERT INTO sharing_expense_legs (id, expense_id, user_id, amount, weight, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&leg_id)
            .bind(&id)
            .bind(user_id)
            .bind(amount)
            .bind(weight)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        // Update balances: payer gains (total_amount - their own leg), each debtor loses their leg
        let leg_amounts: Vec<(String, i64)> = legs
            .iter()
            .map(|(uid, amt, _)| (uid.clone(), *amt))
            .collect();
        self.update_balances(budget_id, paid_by, total_amount, &leg_amounts)
            .await?;

        self.get_expense(&id).await
    }

    pub async fn get_expense(&self, expense_id: &str) -> Result<DbExpense> {
        let row = sqlx::query_as::<_, DbExpense>(
            "SELECT id, budget_id, paid_by, total_amount, description, expense_date,
                    category_id, split_method, created_by, created_at, updated_at
             FROM sharing_expenses WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(expense_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_expenses(&self, budget_id: &str) -> Result<Vec<DbExpense>> {
        let rows = sqlx::query_as::<_, DbExpense>(
            "SELECT id, budget_id, paid_by, total_amount, description, expense_date,
                    category_id, split_method, created_by, created_at, updated_at
             FROM sharing_expenses WHERE budget_id = ? AND deleted_at IS NULL
             ORDER BY expense_date DESC, created_at DESC",
        )
        .bind(budget_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_legs(&self, expense_id: &str) -> Result<Vec<DbExpenseLeg>> {
        let rows = sqlx::query_as::<_, DbExpenseLeg>(
            "SELECT id, expense_id, user_id, amount, weight FROM sharing_expense_legs WHERE expense_id = ?",
        )
        .bind(expense_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // BY_ITEM items + assignments
    // -----------------------------------------------------------------------

    /// Persist the items (and per-item assignments) for a BY_ITEM
    /// expense. Items are written in order; each item gets one row in
    /// `sharing_expense_items` and N rows in
    /// `sharing_expense_item_assignments`.
    pub async fn insert_items(
        &self,
        expense_id: &str,
        items: &[crate::manager::biz::ByItemInput],
    ) -> Result<()> {
        let now = now_unix();
        for it in items {
            let item_id = new_id();
            sqlx::query(
                "INSERT INTO sharing_expense_items
                    (id, expense_id, label, amount, created_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&item_id)
            .bind(expense_id)
            .bind(&it.label)
            .bind(it.amount)
            .bind(now)
            .execute(&self.pool)
            .await?;

            for a in &it.assignments {
                let assign_id = new_id();
                sqlx::query(
                    "INSERT INTO sharing_expense_item_assignments
                        (id, item_id, user_id, numerator)
                     VALUES (?, ?, ?, ?)",
                )
                .bind(&assign_id)
                .bind(&item_id)
                .bind(&a.user_id)
                .bind(a.numerator)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }

    /// Load items + assignments for a single expense. Returns
    /// `(item, assignments)` tuples so callers can map them into the
    /// proto in one shot.
    pub async fn get_items(
        &self,
        expense_id: &str,
    ) -> Result<Vec<(DbExpenseItem, Vec<DbExpenseItemAssignment>)>> {
        let items: Vec<DbExpenseItem> = sqlx::query_as(
            "SELECT id, expense_id, label, amount
             FROM sharing_expense_items
             WHERE expense_id = ?
             ORDER BY created_at ASC, id ASC",
        )
        .bind(expense_id)
        .fetch_all(&self.pool)
        .await?;

        if items.is_empty() {
            return Ok(Vec::new());
        }

        let assigns: Vec<DbExpenseItemAssignment> = sqlx::query_as(
            "SELECT id, item_id, user_id, numerator
             FROM sharing_expense_item_assignments
             WHERE item_id IN (SELECT id FROM sharing_expense_items WHERE expense_id = ?)",
        )
        .bind(expense_id)
        .fetch_all(&self.pool)
        .await?;

        let mut grouped: std::collections::HashMap<String, Vec<DbExpenseItemAssignment>> =
            std::collections::HashMap::new();
        for a in assigns {
            grouped.entry(a.item_id.clone()).or_default().push(a);
        }
        Ok(items
            .into_iter()
            .map(|it| {
                let a = grouped.remove(&it.id).unwrap_or_default();
                (it, a)
            })
            .collect())
    }

    pub async fn delete_expense(&self, expense_id: &str) -> Result<()> {
        let now = now_unix();
        // Reverse balance changes before soft-deleting
        let expense = self.get_expense(expense_id).await?;
        let legs = self.get_legs(expense_id).await?;
        let leg_pairs: Vec<(String, i64)> =
            legs.iter().map(|l| (l.user_id.clone(), l.amount)).collect();
        // Reverse: payer loses, debtors gain
        self.reverse_balances(
            &expense.budget_id,
            &expense.paid_by,
            expense.total_amount,
            &leg_pairs,
        )
        .await?;

        sqlx::query("UPDATE sharing_expenses SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(now)
            .bind(expense_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Balances
    // -----------------------------------------------------------------------

    pub async fn get_balances(&self, budget_id: &str) -> Result<Vec<DbBalance>> {
        // Exclude balance rows for participants that have been revoked.
        // The join is on (budget_id, user_id) so a balance row whose
        // underlying participant was revoked (sharing_participants.revoked_at
        // IS NOT NULL) is filtered out. Balances that have no matching
        // participant row (e.g. legacy data) are also filtered out to avoid
        // showing phantom balances for users no longer in the budget.
        let rows = sqlx::query_as::<_, DbBalance>(
            "SELECT b.user_id, b.net_balance
             FROM sharing_balances b
             INNER JOIN sharing_participants p
               ON p.budget_id = b.budget_id AND p.user_id = b.user_id
             WHERE b.budget_id = ?
               AND p.revoked_at IS NULL
             ORDER BY b.net_balance DESC",
        )
        .bind(budget_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// When an expense is added: payer's balance increases by (total - their own leg),
    /// each debtor's balance decreases by their leg amount.
    async fn update_balances(
        &self,
        budget_id: &str,
        paid_by: &str,
        total_amount: i64,
        legs: &[(String, i64)],
    ) -> Result<()> {
        let now = now_unix();
        // Find payer's own leg (if any)
        let payer_leg = legs
            .iter()
            .find(|(uid, _)| uid == paid_by)
            .map(|(_, a)| *a)
            .unwrap_or(0);
        let payer_credit = total_amount - payer_leg;

        // Credit the payer
        if payer_credit != 0 {
            self.upsert_balance(budget_id, paid_by, payer_credit, now)
                .await?;
        }

        // Debit each debtor (excluding payer's own leg)
        for (user_id, amount) in legs {
            if user_id != paid_by {
                self.upsert_balance(budget_id, user_id, -amount, now)
                    .await?;
            }
        }
        Ok(())
    }

    async fn reverse_balances(
        &self,
        budget_id: &str,
        paid_by: &str,
        total_amount: i64,
        legs: &[(String, i64)],
    ) -> Result<()> {
        let now = now_unix();
        let payer_leg = legs
            .iter()
            .find(|(uid, _)| uid == paid_by)
            .map(|(_, a)| *a)
            .unwrap_or(0);
        let payer_credit = total_amount - payer_leg;

        if payer_credit != 0 {
            self.upsert_balance(budget_id, paid_by, -payer_credit, now)
                .await?;
        }
        for (user_id, amount) in legs {
            if user_id != paid_by {
                self.upsert_balance(budget_id, user_id, *amount, now)
                    .await?;
            }
        }
        Ok(())
    }

    async fn upsert_balance(
        &self,
        budget_id: &str,
        user_id: &str,
        delta: i64,
        now: i64,
    ) -> Result<()> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO sharing_balances (id, budget_id, user_id, net_balance, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON DUPLICATE KEY UPDATE net_balance = net_balance + ?, updated_at = ?",
        )
        .bind(&id)
        .bind(budget_id)
        .bind(user_id)
        .bind(delta)
        .bind(now)
        .bind(delta)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Join links
    // -----------------------------------------------------------------------

    pub async fn create_join_link(
        &self,
        budget_id: &str,
        created_by: &str,
    ) -> Result<(String, String, i64)> {
        let id = new_id();
        let token = uuid::Uuid::new_v4().to_string().replace('-', "");
        let now = now_unix();
        let expires_at = now + 7 * 24 * 3600; // 7 days

        sqlx::query(
            "INSERT INTO sharing_join_links (id, budget_id, token, created_by, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&id).bind(budget_id).bind(&token).bind(created_by).bind(now).bind(expires_at)
        .execute(&self.pool).await?;

        Ok((id, token, expires_at))
    }

    pub async fn get_join_link_budget(&self, token: &str) -> Result<Option<String>> {
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT budget_id, expires_at FROM sharing_join_links WHERE token = ?")
                .bind(token)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.and_then(|(budget_id, expires_at)| {
            if expires_at > now_unix() {
                Some(budget_id)
            } else {
                None
            }
        }))
    }

    /// Like `get_join_link_budget` but also returns the `expires_at`
    /// timestamp, used by the public preview RPC.
    pub async fn get_join_link_budget_with_expires(
        &self,
        token: &str,
    ) -> Result<Option<(String, i64)>> {
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT budget_id, expires_at FROM sharing_join_links WHERE token = ?")
                .bind(token)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.and_then(|(budget_id, expires_at)| {
            if expires_at > now_unix() {
                Some((budget_id, expires_at))
            } else {
                None
            }
        }))
    }

    pub async fn get_join_link_with_creator(
        &self,
        token: &str,
    ) -> Result<Option<(String, String)>> {
        let row: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT budget_id, created_by, expires_at FROM sharing_join_links WHERE token = ?",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(budget_id, created_by, expires_at)| {
            if expires_at > now_unix() {
                Some((budget_id, created_by))
            } else {
                None
            }
        }))
    }

    // -----------------------------------------------------------------------
    // Settlement payments
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn create_payment(
        &self,
        id: &str,
        budget_id: &str,
        from_participant_id: &str,
        to_participant_id: &str,
        amount: i64,
        settled_at: &str,
        note: Option<&str>,
        settled_by_participant_id: &str,
        created_at: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO sharing_settlement_confirmations
             (id, budget_id, from_participant_id, to_participant_id, amount, settled_at, note, settled_by_participant_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(budget_id)
        .bind(from_participant_id)
        .bind(to_participant_id)
        .bind(amount)
        .bind(settled_at)
        .bind(note)
        .bind(settled_by_participant_id)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    pub async fn list_confirmations(
        &self,
        budget_id: &str,
    ) -> Result<
        Vec<(
            String,
            String,
            String,
            String,
            i64,
            String,
            Option<String>,
            String,
            i64,
        )>,
        sqlx::Error,
    > {
        let rows: Vec<(String, String, String, String, i64, String, Option<String>, String, i64)> =
            sqlx::query_as(
                "SELECT id, budget_id, from_participant_id, to_participant_id, amount, CAST(settled_at AS CHAR) AS settled_at, note, settled_by_participant_id, created_at
                 FROM sharing_settlement_confirmations
                 WHERE budget_id = ?
                 ORDER BY settled_at DESC, created_at DESC",
            )
            .bind(budget_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn delete_confirmation(&self, confirmation_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sharing_settlement_confirmations WHERE id = ?")
            .bind(confirmation_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Returns just the (from, to, amount) triples for every confirmed
    /// settlement in this budget. Used by settlement to net them off the
    /// gross balances before running the greedy minimisation.
    pub async fn list_confirmation_amounts(
        &self,
        budget_id: &str,
    ) -> Result<Vec<(String, String, i64)>, sqlx::Error> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT from_participant_id, to_participant_id, amount
             FROM sharing_settlement_confirmations
             WHERE budget_id = ?",
        )
        .bind(budget_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    #[allow(clippy::type_complexity)]
    pub async fn find_duplicate_confirmation(
        &self,
        budget_id: &str,
        from_participant_id: &str,
        to_participant_id: &str,
        amount: i64,
        settled_at: &str,
    ) -> Result<Option<SettlementConfirmation>, sqlx::Error> {
        let row: Option<(String, String, String, String, i64, String, Option<String>, String, i64)> = sqlx::query_as(
            "SELECT id, budget_id, from_participant_id, to_participant_id, amount, CAST(settled_at AS CHAR) AS settled_at, note, settled_by_participant_id, created_at
             FROM sharing_settlement_confirmations
             WHERE budget_id = ? AND from_participant_id = ? AND to_participant_id = ? AND amount = ? AND settled_at = ?
             LIMIT 1",
        )
        .bind(budget_id)
        .bind(from_participant_id)
        .bind(to_participant_id)
        .bind(amount)
        .bind(settled_at)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(
                id,
                budget_id,
                from_participant_id,
                to_participant_id,
                amount,
                settled_at,
                note,
                settled_by_participant_id,
                created_at,
            )| {
                SettlementConfirmation {
                    id,
                    budget_id,
                    from_participant_id,
                    to_participant_id,
                    amount,
                    settled_at,
                    note: note.unwrap_or_default(),
                    settled_by_participant_id,
                    created_at,
                }
            },
        ))
    }

    #[allow(clippy::type_complexity)]
    pub async fn get_confirmation(
        &self,
        confirmation_id: &str,
    ) -> Result<Option<SettlementConfirmation>, sqlx::Error> {
        let row: Option<(String, String, String, String, i64, String, Option<String>, String, i64)> = sqlx::query_as(
            "SELECT id, budget_id, from_participant_id, to_participant_id, amount, CAST(settled_at AS CHAR) AS settled_at, note, settled_by_participant_id, created_at
             FROM sharing_settlement_confirmations
             WHERE id = ?",
        )
        .bind(confirmation_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(
                id,
                budget_id,
                from_participant_id,
                to_participant_id,
                amount,
                settled_at,
                note,
                settled_by_participant_id,
                created_at,
            )| {
                SettlementConfirmation {
                    id,
                    budget_id,
                    from_participant_id,
                    to_participant_id,
                    amount,
                    settled_at,
                    note: note.unwrap_or_default(),
                    settled_by_participant_id,
                    created_at,
                }
            },
        ))
    }

    // -----------------------------------------------------------------------
    // Participants (guest + member)
    // -----------------------------------------------------------------------

    /// Insert a guest participant. `user_id` is the bare id (no prefix
    /// applied here); the caller is expected to use the `g_<uuid>` form.
    /// `session_token_hash` is the SHA-256 hex digest of the session
    /// token. The raw token is never stored.
    pub async fn create_guest_participant(
        &self,
        budget_id: &str,
        user_id: &str,
        display_name: &str,
        session_token_hash: &str,
    ) -> Result<DbParticipant> {
        let id = new_id();
        let now = now_unix();

        sqlx::query(
            "INSERT INTO sharing_participants
                (id, budget_id, participant_kind, user_id, display_name,
                 session_token_hash, joined_at, last_seen_at)
             VALUES (?, ?, 'guest', ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(budget_id)
        .bind(user_id)
        .bind(display_name)
        .bind(session_token_hash)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(DbParticipant {
            id,
            budget_id: budget_id.to_string(),
            participant_kind: "guest".to_string(),
            user_id: Some(user_id.to_string()),
            display_name: display_name.to_string(),
            joined_at: now,
            last_seen_at: now,
            revoked_at: None,
        })
    }

    /// Update the session token (and guest id) for an existing guest row.
    /// Used by the re-join path. Returns true if the row was updated.
    pub async fn rotate_guest_session(
        &self,
        participant_id: &str,
        new_guest_id: &str,
        new_token_hash: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE sharing_participants
             SET user_id = ?, session_token_hash = ?
             WHERE id = ? AND participant_kind = 'guest' AND revoked_at IS NULL",
        )
        .bind(new_guest_id)
        .bind(new_token_hash)
        .bind(participant_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Look up an active guest participant by (budget_id, sha256-hash).
    /// Returns None if no active row matches.
    pub async fn find_guest_by_token(
        &self,
        budget_id: &str,
        session_token_hash: &str,
    ) -> Result<Option<DbParticipant>> {
        let row: Option<DbParticipant> = sqlx::query_as(
            "SELECT id, budget_id, participant_kind, user_id, display_name,
                    joined_at, last_seen_at, revoked_at
             FROM sharing_participants
             WHERE budget_id = ? AND session_token_hash = ?
               AND participant_kind = 'guest' AND revoked_at IS NULL",
        )
        .bind(budget_id)
        .bind(session_token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Look up a member participant by (budget_id, bare-user-id).
    /// A member row is created lazily by `upsert_member_participant`.
    pub async fn find_member_participant(
        &self,
        budget_id: &str,
        user_id: &str,
    ) -> Result<Option<DbParticipant>> {
        let row: Option<DbParticipant> = sqlx::query_as(
            "SELECT id, budget_id, participant_kind, user_id, display_name,
                    joined_at, last_seen_at, revoked_at
             FROM sharing_participants
             WHERE budget_id = ? AND user_id = ?
               AND participant_kind = 'member' AND revoked_at IS NULL",
        )
        .bind(budget_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Look up any active participant (member or guest) by its row id.
    /// Returns None if no active row matches.
    pub async fn find_participant_by_id(
        &self,
        participant_id: &str,
    ) -> Result<Option<DbParticipant>> {
        let row: Option<DbParticipant> = sqlx::query_as(
            "SELECT id, budget_id, participant_kind, user_id, display_name,
                    joined_at, last_seen_at, revoked_at
             FROM sharing_participants
             WHERE id = ? AND revoked_at IS NULL",
        )
        .bind(participant_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Look up all active guest participants matching the SHA-256 hash
    /// of the presented session token. In practice this should match
    /// exactly one row (or zero). Used as a fallback when the gateway
    /// did not inject `x-budget-id`.
    pub async fn list_active_guests_by_hash(
        &self,
        session_token_hash: &str,
    ) -> Result<Vec<DbParticipant>> {
        let rows: Vec<DbParticipant> = sqlx::query_as(
            "SELECT id, budget_id, participant_kind, user_id, display_name,
                    joined_at, last_seen_at, revoked_at
             FROM sharing_participants
             WHERE session_token_hash = ?
               AND participant_kind = 'guest' AND revoked_at IS NULL",
        )
        .bind(session_token_hash)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Idempotent insert for a member participant. Called when a Normal
    /// User with a Budget role on the parent budget first interacts with
    /// the sharing service.
    pub async fn upsert_member_participant(
        &self,
        budget_id: &str,
        user_id: &str,
        display_name: &str,
    ) -> Result<DbParticipant> {
        let now = now_unix();
        sqlx::query(
            "INSERT INTO sharing_participants
                (id, budget_id, participant_kind, user_id, display_name,
                 session_token_hash, joined_at, last_seen_at)
             VALUES (?, ?, 'member', ?, ?, NULL, ?, ?)
             ON DUPLICATE KEY UPDATE last_seen_at = VALUES(last_seen_at)",
        )
        .bind(new_id())
        .bind(budget_id)
        .bind(user_id)
        .bind(display_name)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Re-read so the returned struct is current.
        let row = sqlx::query_as::<_, DbParticipant>(
            "SELECT id, budget_id, participant_kind, user_id, display_name,
                    joined_at, last_seen_at, revoked_at
             FROM sharing_participants
             WHERE budget_id = ? AND user_id = ? AND participant_kind = 'member'",
        )
        .bind(budget_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn touch_last_seen(&self, participant_id: &str) -> Result<()> {
        let now = now_unix();
        sqlx::query("UPDATE sharing_participants SET last_seen_at = ? WHERE id = ?")
            .bind(now)
            .bind(participant_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn revoke_participant(&self, budget_id: &str, participant_id: &str) -> Result<bool> {
        let now = now_unix();
        let result = sqlx::query(
            "UPDATE sharing_participants
             SET revoked_at = ? WHERE id = ? AND budget_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(participant_id)
        .bind(budget_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_participants(&self, budget_id: &str) -> Result<Vec<DbParticipant>> {
        let rows: Vec<DbParticipant> = sqlx::query_as(
            "SELECT id, budget_id, participant_kind, user_id, display_name,
                    joined_at, last_seen_at, revoked_at
             FROM sharing_participants
             WHERE budget_id = ? AND revoked_at IS NULL
             ORDER BY participant_kind DESC, joined_at ASC",
        )
        .bind(budget_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Per-expense comments
    // -----------------------------------------------------------------------

    pub async fn create_comment(
        &self,
        expense_id: &str,
        author_participant_id: &str,
        author_display_name: &str,
        body: &str,
        created_at: i64,
    ) -> Result<crate::pb::service::sharing::ExpenseComment, sqlx::Error> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO sharing_expense_comments
                (id, expense_id, author_participant_id, author_display_name, body, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(expense_id)
        .bind(author_participant_id)
        .bind(author_display_name)
        .bind(body)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(crate::pb::service::sharing::ExpenseComment {
            id,
            expense_id: expense_id.to_string(),
            author_participant_id: author_participant_id.to_string(),
            author_display_name: author_display_name.to_string(),
            body: body.to_string(),
            created_at,
            deleted: false,
        })
    }

    pub async fn list_comments(
        &self,
        expense_id: &str,
    ) -> Result<Vec<crate::pb::service::sharing::ExpenseComment>, sqlx::Error> {
        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, String, String, String, String, i64, Option<i64>)> = sqlx::query_as(
            "SELECT id, expense_id, author_participant_id, author_display_name, body, created_at, deleted_at
             FROM sharing_expense_comments
             WHERE expense_id = ? AND deleted_at IS NULL
             ORDER BY created_at ASC",
        )
        .bind(expense_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, expense_id, author, author_name, body, created_at, _deleted)| {
                    crate::pb::service::sharing::ExpenseComment {
                        id,
                        expense_id,
                        author_participant_id: author,
                        author_display_name: author_name,
                        body,
                        created_at,
                        deleted: false,
                    }
                },
            )
            .collect())
    }

    pub async fn delete_comment(&self, comment_id: &str) -> Result<bool, sqlx::Error> {
        let now = now_unix();
        let result = sqlx::query(
            "UPDATE sharing_expense_comments
             SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(comment_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn find_comment(
        &self,
        comment_id: &str,
    ) -> Result<Option<(String, String)>, sqlx::Error> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT author_participant_id, expense_id
             FROM sharing_expense_comments
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(comment_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn get_expense_budget_id(
        &self,
        expense_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT budget_id FROM sharing_expenses WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(expense_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(b,)| b))
    }

    // -----------------------------------------------------------------------
    // Activity log
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn record_activity(
        &self,
        budget_id: &str,
        actor_participant_id: &str,
        actor_display_name: &str,
        action: &str,
        target_type: &str,
        target_id: &str,
        metadata_json: &str,
        created_at: i64,
    ) -> Result<(), sqlx::Error> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO sharing_activity_log
                (id, budget_id, actor_participant_id, actor_display_name, action,
                 target_type, target_id, metadata_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(budget_id)
        .bind(actor_participant_id)
        .bind(actor_display_name)
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(metadata_json)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_activity(
        &self,
        budget_id: &str,
        since_unix: i64,
        limit: i32,
    ) -> Result<Vec<crate::pb::service::sharing::ActivityLogEntry>, sqlx::Error> {
        let lim = if limit <= 0 { 50 } else { limit.min(500) };
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            i64,
        )> = if since_unix > 0 {
            sqlx::query_as(
                "SELECT id, budget_id, actor_participant_id, actor_display_name, action,
                        target_type, target_id, metadata_json, created_at
                 FROM sharing_activity_log
                 WHERE budget_id = ? AND created_at >= ?
                 ORDER BY created_at DESC LIMIT ?",
            )
            .bind(budget_id)
            .bind(since_unix)
            .bind(lim)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT id, budget_id, actor_participant_id, actor_display_name, action,
                        target_type, target_id, metadata_json, created_at
                 FROM sharing_activity_log
                 WHERE budget_id = ?
                 ORDER BY created_at DESC LIMIT ?",
            )
            .bind(budget_id)
            .bind(lim)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    budget_id,
                    actor,
                    actor_name,
                    action,
                    target_type,
                    target_id,
                    metadata_json,
                    created_at,
                )| {
                    crate::pb::service::sharing::ActivityLogEntry {
                        id,
                        budget_id,
                        actor_participant_id: actor,
                        actor_display_name: actor_name,
                        action,
                        target_type,
                        target_id,
                        metadata_json,
                        created_at,
                    }
                },
            )
            .collect())
    }

    /// All distinct budget_ids that have any sharing activity
    /// (participants, expenses, or join links). Used by the backfill
    /// CLI to enumerate every sharing budget without scanning tables
    /// the service otherwise ignores.
    pub async fn distinct_budget_ids_with_activity(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT budget_id FROM sharing_participants
             UNION
             SELECT DISTINCT budget_id FROM sharing_expenses
             UNION
             SELECT DISTINCT budget_id FROM sharing_join_links",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Pick any active participant user_id for a budget. Used by the
    /// backfill CLI as the auth subject for the budget service's
    /// `ListBudgetMembers` call, which requires the caller to be a
    /// member of the budget. Member rows are preferred; falls back to
    /// any active row (members, then guests) if no member exists.
    pub async fn any_active_user_id(&self, budget_id: &str) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT user_id FROM sharing_participants
             WHERE budget_id = ? AND revoked_at IS NULL AND user_id IS NOT NULL
             ORDER BY (participant_kind = 'member') DESC, joined_at ASC
             LIMIT 1",
        )
        .bind(budget_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(u,)| u))
    }
}
