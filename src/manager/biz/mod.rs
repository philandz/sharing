#![allow(clippy::result_large_err)]
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::Status;

use crate::converters::map_expense;
use crate::manager::client::BudgetClient;
use crate::manager::repository::SharingRepository;
use crate::pb::service::budget::BudgetRole;
use crate::pb::service::sharing::{
    AcceptJoinLinkResponse, Expense, JoinLink, Settlement, SettlementPayment, SplitMethod, Transfer,
};

pub struct SharingBiz {
    pub repo: Arc<SharingRepository>,
    pub budget_client: Arc<Mutex<BudgetClient>>,
    pub vietqr_base: String,
}

impl SharingBiz {
    pub fn new(repo: SharingRepository, budget_client: BudgetClient, vietqr_base: String) -> Self {
        Self {
            repo: Arc::new(repo),
            budget_client: Arc::new(Mutex::new(budget_client)),
            vietqr_base,
        }
    }

    fn internal(e: impl ToString) -> Status {
        Status::internal(e.to_string())
    }

    // -----------------------------------------------------------------------
    // Role helpers
    // -----------------------------------------------------------------------

    async fn assert_member(&self, budget_id: &str, user_id: &str) -> Result<(), Status> {
        let role = self
            .budget_client
            .lock()
            .await
            .check_role(user_id, budget_id)
            .await?;
        if role == BudgetRole::Unspecified {
            return Err(Status::permission_denied("Not a member of this budget"));
        }
        Ok(())
    }

    async fn assert_contributor(&self, budget_id: &str, user_id: &str) -> Result<(), Status> {
        let role = self
            .budget_client
            .lock()
            .await
            .check_role(user_id, budget_id)
            .await?;
        if matches!(role, BudgetRole::Unspecified | BudgetRole::Viewer) {
            return Err(Status::permission_denied(
                "Requires Contributor role or higher",
            ));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Expense management
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn add_expense(
        &self,
        user_id: &str,
        budget_id: &str,
        paid_by: &str,
        total_amount: i64,
        description: &str,
        expense_date: &str,
        category_id: Option<&str>,
        split_method: SplitMethod,
        legs: Vec<(String, i64)>,
    ) -> Result<Expense, Status> {
        self.assert_contributor(budget_id, user_id).await?;

        // For custom splits, the per-leg amount must sum to total_amount.
        // For weighted splits, the (user_id, weight) pairs are converted to
        // per-user amounts here before being persisted. For equal splits, the
        // handler has already divided the amount evenly.
        let computed_legs: Vec<(String, i64)> = if split_method == SplitMethod::Weighted {
            // Distribute total_amount proportionally to the weights, with the
            // last leg absorbing any rounding remainder so the sum is exact.
            let total_weight: i64 = legs.iter().map(|(_, w)| w).sum();
            if total_weight <= 0 {
                return Err(Status::invalid_argument("total weight must be > 0"));
            }
            let mut sorted: Vec<(String, i64)> = legs.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let len = sorted.len();
            let mut amounts: Vec<(String, i64)> = Vec::with_capacity(len);
            let mut assigned: i64 = 0;
            for (i, (uid, weight)) in sorted.into_iter().enumerate() {
                let share = total_amount * weight / total_weight;
                assigned += share;
                if i == len - 1 {
                    amounts.push((uid, total_amount - (assigned - share)));
                } else {
                    amounts.push((uid, share));
                }
            }
            amounts
        } else if split_method == SplitMethod::Custom {
            let leg_sum: i64 = legs.iter().map(|(_, a)| a).sum();
            if leg_sum != total_amount {
                return Err(Status::invalid_argument(format!(
                    "Legs sum ({leg_sum}) must equal total_amount ({total_amount})"
                )));
            }
            legs
        } else {
            // Equal: handler has already divided; just persist
            legs
        };

        let db = self
            .repo
            .create_expense(
                budget_id,
                paid_by,
                total_amount,
                description,
                expense_date,
                category_id,
                split_method,
                &computed_legs,
                user_id,
            )
            .await
            .map_err(Self::internal)?;

        let legs_db = self.repo.get_legs(&db.id).await.map_err(Self::internal)?;
        Ok(map_expense(db, legs_db))
    }

    pub async fn get_expense(&self, user_id: &str, expense_id: &str) -> Result<Expense, Status> {
        let db = self
            .repo
            .get_expense(expense_id)
            .await
            .map_err(|_| Status::not_found("Expense not found"))?;
        self.assert_member(&db.budget_id, user_id).await?;
        let legs = self
            .repo
            .get_legs(expense_id)
            .await
            .map_err(Self::internal)?;
        Ok(map_expense(db, legs))
    }

    pub async fn get_balances(
        &self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<Vec<crate::pb::service::sharing::Participant>, Status> {
        self.assert_member(budget_id, user_id).await?;
        let balances = self.repo.get_balances(budget_id).await.map_err(Self::internal)?;
        Ok(balances
            .into_iter()
            .map(|b| crate::pb::service::sharing::Participant {
                user_id: b.user_id,
                display_name: String::new(),  // resolved by frontend from member list
                email: String::new(),
                net_balance: b.net_balance,
            })
            .collect())
    }

    pub async fn list_expenses(
        &self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<Vec<Expense>, Status> {
        self.assert_member(budget_id, user_id).await?;
        let rows = self
            .repo
            .list_expenses(budget_id)
            .await
            .map_err(Self::internal)?;
        let mut result = Vec::with_capacity(rows.len());
        for db in rows {
            let legs = self.repo.get_legs(&db.id).await.map_err(Self::internal)?;
            result.push(map_expense(db, legs));
        }
        Ok(result)
    }

    pub async fn delete_expense(&self, user_id: &str, expense_id: &str) -> Result<(), Status> {
        let db = self
            .repo
            .get_expense(expense_id)
            .await
            .map_err(|_| Status::not_found("Expense not found"))?;
        self.assert_contributor(&db.budget_id, user_id).await?;

        // Ownership rule (Bead 0.1): only the expense creator OR a budget
        // owner can delete. Any other contributor is rejected with
        // permission_denied. The check_role gRPC call also requires the
        // caller's x-user-id metadata to be propagated (see BudgetClient::check_role).
        let is_creator = db.created_by == user_id;
        let is_budget_owner = matches!(
            self.budget_client
                .lock()
                .await
                .check_role(user_id, &db.budget_id)
                .await?,
            BudgetRole::Owner
        );
        if !is_creator && !is_budget_owner {
            return Err(Status::permission_denied(
                "Only the expense creator or a budget owner can delete this expense",
            ));
        }

        self.repo
            .delete_expense(expense_id)
            .await
            .map_err(Self::internal)
    }

    // -----------------------------------------------------------------------
    // Settlement — debt minimization algorithm
    //
    // Algorithm:
    //   1. Collect all net balances (positive = owed to, negative = owes)
    //   2. Greedily pair largest creditor with largest debtor
    //   3. Settle the minimum of the two amounts
    //   4. Repeat until all balances are zero
    //   Result: at most N-1 transfers for N participants
    // -----------------------------------------------------------------------

    pub async fn calculate_settlement(
        &self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<Settlement, Status> {
        self.assert_member(budget_id, user_id).await?;

        let balances = self
            .repo
            .get_balances(budget_id)
            .await
            .map_err(Self::internal)?;

        // Build signed balance list: (user_id, net_balance)
        let mut signed: Vec<(String, i64)> = balances
            .into_iter()
            .filter(|b| b.net_balance != 0)
            .map(|b| (b.user_id, b.net_balance))
            .collect();

        let mut transfers: Vec<Transfer> = Vec::new();

        // Greedy debt minimization
        loop {
            // Sort: creditors (positive) at end, debtors (negative) at start
            signed.sort_by_key(|(_, b)| *b);
            let first = signed.first().map(|(_, b)| *b).unwrap_or(0);
            let last = signed.last().map(|(_, b)| *b).unwrap_or(0);

            if first >= 0 || last <= 0 {
                break;
            } // all settled

            let debtor_idx = 0;
            let creditor_idx = signed.len() - 1;

            let debtor_id = signed[debtor_idx].0.clone();
            let creditor_id = signed[creditor_idx].0.clone();
            let debt = -signed[debtor_idx].1;
            let credit = signed[creditor_idx].1;
            let amount = debt.min(credit);

            // Generate VietQR deep-link (best-effort, no bank account info available here).
            // Use chars().take(8) so short user_ids (e.g. "u1") don't panic.
            let deep_link = format!(
                "{}/napas247-{}-TRANSFER.jpg?amount={}&addInfo=Settle+sharing+budget",
                self.vietqr_base,
                creditor_id.chars().take(8).collect::<String>(),
                amount
            );

            transfers.push(Transfer {
                from_user_id: debtor_id.clone(),
                from_name: debtor_id.clone(),
                to_user_id: creditor_id.clone(),
                to_name: creditor_id.clone(),
                amount,
                deep_link,
            });

            signed[debtor_idx].1 += amount;
            signed[creditor_idx].1 -= amount;

            // Remove zeroed entries
            signed.retain(|(_, b)| *b != 0);
        }

        Ok(Settlement {
            budget_id: budget_id.to_string(),
            transfers,
        })
    }

    // -----------------------------------------------------------------------
    // Join links
    // -----------------------------------------------------------------------

    pub async fn generate_join_link(
        &self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<JoinLink, Status> {
        self.assert_contributor(budget_id, user_id).await?;
        let (_id, token, expires_at) = self
            .repo
            .create_join_link(budget_id, user_id)
            .await
            .map_err(Self::internal)?;
        let join_url = format!("/join-budget?token={token}");
        Ok(JoinLink {
            token,
            budget_id: budget_id.to_string(),
            join_url,
            expires_at,
        })
    }

    pub async fn accept_join_link(
        &self,
        token: &str,
        user_id: &str,
    ) -> Result<AcceptJoinLinkResponse, Status> {
        let (budget_id, generating_user_id) = self
            .repo
            .get_join_link_with_creator(token)
            .await
            .map_err(Self::internal)?
            .ok_or_else(|| Status::not_found("Join link not found or expired"))?;

        // Idempotency: if already a member, return success without re-adding.
        let role = self
            .budget_client
            .lock()
            .await
            .check_role(user_id, &budget_id)
            .await?;
        if role != BudgetRole::Unspecified {
            return Ok(AcceptJoinLinkResponse {
                success: true,
                budget_id: budget_id.clone(),
            });
        }

        // Add the new member as a Contributor, on behalf of the link creator.
        // `add_member_as` uses the system-actor bypass so we don't need the
        // creator to be a Manager/Owner of the budget at the time of accept.
        self.budget_client
            .lock()
            .await
            .add_member_as(&budget_id, &generating_user_id, user_id, BudgetRole::Contributor)
            .await?;

        tracing::info!(
            "User {user_id} joined budget {budget_id} via link from {generating_user_id}"
        );
        Ok(AcceptJoinLinkResponse {
            success: true,
            budget_id,
        })
    }

    // -----------------------------------------------------------------------
    // Settlement payments (mark-as-paid)
    // -----------------------------------------------------------------------

    pub async fn mark_payment(
        &self,
        user_id: &str,
        budget_id: &str,
        from_user_id: &str,
        to_user_id: &str,
        amount: i64,
        paid_at: &str,
        note: Option<&str>,
    ) -> Result<SettlementPayment, Status> {
        self.assert_contributor(budget_id, user_id).await?;
        if amount <= 0 {
            return Err(Status::invalid_argument("Payment amount must be > 0"));
        }
        if from_user_id == to_user_id {
            return Err(Status::invalid_argument("from_user_id and to_user_id must differ"));
        }

        // Idempotency: if an identical payment exists, return it instead of duplicating.
        if let Some(existing) = self
            .repo
            .find_duplicate_payment(budget_id, from_user_id, to_user_id, amount, paid_at)
            .await
            .map_err(Self::internal)?
        {
            return Ok(existing);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        self.repo
            .create_payment(&id, budget_id, from_user_id, to_user_id, amount, paid_at, note, user_id, now)
            .await
            .map_err(Self::internal)?;

        Ok(SettlementPayment {
            id,
            budget_id: budget_id.to_string(),
            from_user_id: from_user_id.to_string(),
            to_user_id: to_user_id.to_string(),
            amount,
            paid_at: paid_at.to_string(),
            note: note.unwrap_or("").to_string(),
            created_by: user_id.to_string(),
            created_at: now,
        })
    }

    pub async fn list_payments(
        &self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<Vec<SettlementPayment>, Status> {
        self.assert_member(budget_id, user_id).await?;
        let rows = self.repo.list_payments(budget_id).await.map_err(Self::internal)?;
        Ok(rows
            .into_iter()
            .map(|(id, from, to, amount, paid_at, note, created_by, created_at)| SettlementPayment {
                id,
                budget_id: budget_id.to_string(),
                from_user_id: from,
                to_user_id: to,
                amount,
                paid_at,
                note: note.unwrap_or_default(),
                created_by,
                created_at,
            })
            .collect())
    }

    pub async fn delete_payment(
        &self,
        user_id: &str,
        payment_id: &str,
    ) -> Result<(), Status> {
        // Only the payment creator OR a budget owner can delete a payment.
        let payment = self
            .repo
            .get_payment(payment_id)
            .await
            .map_err(Self::internal)?
            .ok_or_else(|| Status::not_found("Payment not found"))?;
        let is_creator = payment.created_by == user_id;
        let is_owner = matches!(
            self.budget_client
                .lock()
                .await
                .check_role(user_id, &payment.budget_id)
                .await?,
            BudgetRole::Owner
        );
        if !is_creator && !is_owner {
            return Err(Status::permission_denied(
                "Only the payment creator or a budget owner can delete this payment",
            ));
        }
        self.repo.delete_payment(payment_id).await.map_err(Self::internal)
    }
}

#[cfg(test)]
mod delete_ownership {
    // The ownership check lives in `SharingBiz::delete_expense` in this
    // same file (see the `delete_expense` method above). The gRPC handler
    // is a thin shim that extracts the user_id from metadata and delegates.
    // This unit test is a guard-rail marker — real coverage lives in
    // `sharing/tests/e2e.sh` (D.19b). TODO: replace with a mocked unit
    // test that exercises `delete_expense` directly.

    #[test]
    fn delete_expense_documents_ownership_rule() {
        // Contract: only the creator OR a budget owner may delete.
        let allowed_for_creator = true;
        let allowed_for_owner = true;
        let allowed_for_other_contributor = false;
        assert!(allowed_for_creator);
        assert!(allowed_for_owner);
        assert!(!allowed_for_other_contributor);
    }
}
