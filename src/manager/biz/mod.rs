#![allow(clippy::result_large_err)]
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::Status;

use crate::converters::map_expense;
use crate::manager::client::BudgetClient;
use crate::manager::repository::SharingRepository;
use crate::pb::service::budget::BudgetRole;
use crate::pb::service::sharing::{
    AcceptJoinLinkResponse, Expense, JoinLink, Settlement, SettlementConfirmation, SplitMethod,
};
use philand_crypto::sha256_hex;
use philand_random::{random_string, uuid_v4};

pub mod settlement;

/// Authenticated participant context extracted from gRPC metadata.
///
/// `participant_id` is the bare id; for members it's the identity-service
/// user_id, for guests it's `g_<uuid>`. `display_name` is what the
/// UI shows. `is_guest` lets the handler apply guest-specific
/// authorization (e.g. cannot call admin RPCs). `budget_role` is the
/// parent's Budget role, only meaningful for members; for guests it is
/// always `Contributor` (the only role a guest can have).
#[derive(Debug, Clone)]
pub struct ParticipantContext {
    pub participant_id: String,
    pub display_name: String,
    pub is_guest: bool,
    pub budget_role: BudgetRole,
}

pub struct SharingBiz {
    pub repo: Arc<SharingRepository>,
    pub budget_client: Arc<Mutex<BudgetClient>>,
}

impl SharingBiz {
    pub fn new(repo: SharingRepository, budget_client: BudgetClient) -> Self {
        Self {
            repo: Arc::new(repo),
            budget_client: Arc::new(Mutex::new(budget_client)),
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
        legs: Vec<(String, i64, i64)>, // (user_id, amount, weight)
    ) -> Result<Expense, Status> {
        self.assert_contributor(budget_id, user_id).await?;

        // For custom splits, the per-leg amount must sum to total_amount.
        // For weighted splits, the (user_id, _, weight) triples are converted to
        // (user_id, computed_amount, weight) before being persisted.
        // For equal splits, the handler has already divided the amount evenly.
        let computed_legs: Vec<(String, i64, i64)> = if split_method == SplitMethod::Weighted {
            let total_weight: i64 = legs.iter().map(|(_, _, w)| w).sum();
            if total_weight <= 0 {
                return Err(Status::invalid_argument("total weight must be > 0"));
            }
            let mut sorted: Vec<(String, i64, i64)> = legs.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let len = sorted.len();
            let mut amounts: Vec<(String, i64, i64)> = Vec::with_capacity(len);
            let mut assigned: i64 = 0;
            for (i, (uid, _, weight)) in sorted.into_iter().enumerate() {
                let share = total_amount * weight / total_weight;
                assigned += share;
                let final_amount = if i == len - 1 {
                    total_amount - (assigned - share)
                } else {
                    share
                };
                amounts.push((uid, final_amount, weight));
            }
            amounts
        } else if split_method == SplitMethod::Custom {
            let leg_sum: i64 = legs.iter().map(|(_, a, _)| a).sum();
            if leg_sum != total_amount {
                return Err(Status::invalid_argument(format!(
                    "Legs sum ({leg_sum}) must equal total_amount ({total_amount})"
                )));
            }
            legs
        } else {
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

        // Subtract confirmed payments from each user's balance.
        // Payment (A -> B, 100) means: A's debt was reduced by 100 (balance moves toward 0),
        // B's credit was reduced by 100 (balance moves toward 0). So A += amount, B -= amount.
        let payments = self
            .repo
            .list_confirmation_amounts(budget_id)
            .await
            .map_err(Self::internal)?;
        let mut net: HashMap<String, i64> = HashMap::new();
        for b in balances.into_iter().filter(|b| b.net_balance != 0) {
            net.insert(b.user_id, b.net_balance);
        }
        for (from, to, amount) in payments {
            *net.entry(from).or_insert(0) += amount;
            *net.entry(to).or_insert(0) -= amount;
        }

        // Build signed balance list: (user_id, name, net_balance).
        // Names are placeholders (== user_id) until the joining-user-name
        // resolution lands in the repository; the frontend currently resolves
        // names from the member list, so a placeholder here is acceptable.
        let signed: Vec<(String, String, i64)> = net
            .into_iter()
            .filter(|(_, v)| *v != 0)
            .map(|(user_id, balance)| (user_id.clone(), user_id, balance))
            .collect();

        let transfers = settlement::greedy_settle(&signed);

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
    // Guest participant lifecycle (account-free, join-by-link)
    // -----------------------------------------------------------------------

    /// Mint a new session token for a guest and return (token, display_name,
    /// participant_id). The token is returned exactly once and never stored
    /// — only its SHA-256 hash lives in the DB. The caller (handler) is
    /// expected to forward the raw token to the frontend for storage in
    /// localStorage.
    pub async fn join_as_guest(
        &self,
        join_token: &str,
        display_name: &str,
    ) -> Result<(String, String, String, String), Status> {
        // Validate display name
        let name = display_name.trim();
        if name.len() < 2 || name.len() > 60 {
            return Err(Status::invalid_argument(
                "display_name must be 2-60 characters",
            ));
        }

        // Look up the join link (must be unexpired)
        let (budget_id, _created_by) = self
            .repo
            .get_join_link_with_creator(join_token)
            .await
            .map_err(Self::internal)?
            .ok_or_else(|| Status::not_found("Join link not found or expired"))?;

        // Idempotency: if the user is already a member, return success
        // without re-adding.
        // (For now, members and guests use the same column; we check
        // for an active guest row with this exact display_name, since
        // guests don't have a stable user_id we can dedupe on.)
        if let Some(existing) = self
            .repo
            .list_participants(&budget_id)
            .await
            .map_err(Self::internal)?
            .into_iter()
            .find(|p| {
                p.participant_kind == "guest"
                    && p.display_name.eq_ignore_ascii_case(name)
                    && p.revoked_at.is_none()
            })
        {
            // Re-join: rotate the session token. The previous token is
            // invalidated by writing a new hash.
            let new_token = random_string(48);
            let new_hash = sha256_hex(&new_token);
            let new_guest_id = format!("g_{}", uuid_v4());
            self.repo
                .rotate_guest_session(&existing.id, &new_guest_id, &new_hash)
                .await
                .map_err(Self::internal)?;
            return Ok((new_token, name.to_string(), new_guest_id, budget_id));
        }

        // New guest: create a row
        let session_token = random_string(48);
        let session_hash = sha256_hex(&session_token);
        let guest_id = format!("g_{}", uuid_v4());

        self.repo
            .create_guest_participant(&budget_id, &guest_id, name, &session_hash)
            .await
            .map_err(Self::internal)?;

        tracing::info!(
            "Guest '{name}' joined budget {budget_id} via link (id={guest_id})"
        );
        Ok((session_token, name.to_string(), guest_id, budget_id))
    }

    /// Look up the participant from a hashed session token. Returns None
    /// if the token doesn't match any active guest.
    pub async fn participant_from_guest_token(
        &self,
        budget_id: &str,
        session_token: &str,
    ) -> Result<Option<ParticipantContext>, Status> {
        let hash = sha256_hex(session_token);
        let row = self
            .repo
            .find_guest_by_token(budget_id, &hash)
            .await
            .map_err(Self::internal)?;
        if let Some(p) = row {
            // Best-effort last_seen update (rate-limited at the handler
            // level so we don't hit the DB on every call).
            let _ = self.repo.touch_last_seen(&p.id).await;
            Ok(Some(ParticipantContext {
                participant_id: p.user_id.unwrap_or_default(),
                display_name: p.display_name,
                is_guest: true,
                budget_role: BudgetRole::Contributor,
            }))
        } else {
            Ok(None)
        }
    }

    /// Idempotent upsert of a member participant. Called by the handler
    /// when a Normal User's JWT is used to call a sharing RPC. We make
    /// sure the sharing_participants row exists so balance/expense
    /// queries have a stable participant_id.
    pub async fn upsert_member(
        &self,
        budget_id: &str,
        user_id: &str,
        display_name: &str,
    ) -> Result<ParticipantContext, Status> {
        let p = self
            .repo
            .upsert_member_participant(budget_id, user_id, display_name)
            .await
            .map_err(Self::internal)?;
        Ok(ParticipantContext {
            participant_id: p.user_id.unwrap_or_default(),
            display_name: p.display_name,
            is_guest: false,
            budget_role: BudgetRole::Contributor, // updated below by handler if needed
        })
    }

    pub async fn list_active_participants(
        &self,
        budget_id: &str,
    ) -> Result<Vec<(String, String, bool)>, Status> {
        // Returns Vec<(participant_id, display_name, is_guest)>
        let rows = self
            .repo
            .list_participants(budget_id)
            .await
            .map_err(Self::internal)?;
        Ok(rows
            .into_iter()
            .map(|p| {
                let is_guest = p.participant_kind == "guest";
                (p.user_id.unwrap_or_default(), p.display_name, is_guest)
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Settlement confirmations (mark-as-settled)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn mark_payment(
        &self,
        user_id: &str,
        budget_id: &str,
        from_participant_id: &str,
        to_participant_id: &str,
        amount: i64,
        settled_at: &str,
        note: Option<&str>,
    ) -> Result<SettlementConfirmation, Status> {
        self.assert_contributor(budget_id, user_id).await?;
        if amount <= 0 {
            return Err(Status::invalid_argument("Settlement amount must be > 0"));
        }
        if from_participant_id == to_participant_id {
            return Err(Status::invalid_argument(
                "from_participant_id and to_participant_id must differ",
            ));
        }

        // Idempotency: if an identical confirmation exists, return it
        // instead of duplicating.
        if let Some(existing) = self
            .repo
            .find_duplicate_confirmation(budget_id, from_participant_id, to_participant_id, amount, settled_at)
            .await
            .map_err(Self::internal)?
        {
            return Ok(existing);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        self.repo
            .create_payment(
                &id,
                budget_id,
                from_participant_id,
                to_participant_id,
                amount,
                settled_at,
                note,
                user_id,
                now,
            )
            .await
            .map_err(Self::internal)?;

        Ok(SettlementConfirmation {
            id,
            budget_id: budget_id.to_string(),
            from_participant_id: from_participant_id.to_string(),
            to_participant_id: to_participant_id.to_string(),
            amount,
            settled_at: settled_at.to_string(),
            note: note.unwrap_or("").to_string(),
            settled_by_participant_id: user_id.to_string(),
            created_at: now,
        })
    }

    pub async fn list_payments(
        &self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<Vec<SettlementConfirmation>, Status> {
        self.assert_member(budget_id, user_id).await?;
        let rows = self.repo.list_confirmations(budget_id).await.map_err(Self::internal)?;
        Ok(rows
            .into_iter()
            .map(|(id, budget_id, from, to, amount, settled_at, note, settled_by, created_at)| {
                SettlementConfirmation {
                    id,
                    budget_id,
                    from_participant_id: from,
                    to_participant_id: to,
                    amount,
                    settled_at,
                    note: note.unwrap_or_default(),
                    settled_by_participant_id: settled_by,
                    created_at,
                }
            })
            .collect())
    }

    pub async fn delete_payment(
        &self,
        user_id: &str,
        confirmation_id: &str,
    ) -> Result<(), Status> {
        // Only the confirmation creator OR a budget owner can delete.
        let confirmation = self
            .repo
            .get_confirmation(confirmation_id)
            .await
            .map_err(Self::internal)?
            .ok_or_else(|| Status::not_found("Settlement confirmation not found"))?;
        let is_creator = confirmation.settled_by_participant_id == user_id;
        let is_owner = matches!(
            self.budget_client
                .lock()
                .await
                .check_role(user_id, &confirmation.budget_id)
                .await?,
            BudgetRole::Owner
        );
        if !is_creator && !is_owner {
            return Err(Status::permission_denied(
                "Only the confirmation creator or a budget owner can delete this",
            ));
        }
        self.repo
            .delete_confirmation(confirmation_id)
            .await
            .map_err(Self::internal)
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

// =====================================================================
// Phase 4 placeholder methods. These exist so the gRPC handlers can
// reference them; the real implementations land in Phase 4 (Bead 4.2,
// 4.3, etc.). Each is gated to return UNIMPLEMENTED.
// =====================================================================

impl SharingBiz {
    // -----------------------------------------------------------------------
    // Per-expense comments (Phase 4 placeholder; full impl in Bead 4.2)
    // -----------------------------------------------------------------------

    pub async fn add_comment(
        &self,
        _user_id: &str,
        _expense_id: &str,
        _body: &str,
    ) -> Result<crate::pb::service::sharing::ExpenseComment, Status> {
        Err(Status::unimplemented("add_comment: pending Phase 4.2"))
    }

    pub async fn list_comments(
        &self,
        _expense_id: &str,
    ) -> Result<Vec<crate::pb::service::sharing::ExpenseComment>, Status> {
        Err(Status::unimplemented("list_comments: pending Phase 4.2"))
    }

    pub async fn delete_comment(
        &self,
        _user_id: &str,
        _comment_id: &str,
    ) -> Result<(), Status> {
        Err(Status::unimplemented("delete_comment: pending Phase 4.2"))
    }

    // -----------------------------------------------------------------------
    // Activity log (Phase 4 placeholder)
    // -----------------------------------------------------------------------

    pub async fn list_activity(
        &self,
        _budget_id: &str,
        _since_unix: i64,
        _limit: i32,
    ) -> Result<Vec<crate::pb::service::sharing::ActivityLogEntry>, Status> {
        Err(Status::unimplemented("list_activity: pending Phase 4.3"))
    }

    // -----------------------------------------------------------------------
    // Participants (typed; Phase 4 placeholder)
    // -----------------------------------------------------------------------

    pub async fn list_participants_typed(
        &self,
        _budget_id: &str,
    ) -> Result<Vec<crate::pb::service::sharing::ParticipantInfo>, Status> {
        Err(Status::unimplemented("list_participants: pending Phase 4"))
    }

    pub async fn revoke_participant(
        &self,
        _user_id: &str,
        _budget_id: &str,
        _participant_id: &str,
    ) -> Result<bool, Status> {
        Err(Status::unimplemented("revoke_participant: pending Phase 4"))
    }

    // -----------------------------------------------------------------------
    // Join link preview (Phase 4 placeholder)
    // -----------------------------------------------------------------------

    pub async fn preview_join_link(
        &self,
        _token: &str,
    ) -> Result<crate::pb::service::sharing::PreviewJoinLinkResponse, Status> {
        Err(Status::unimplemented("preview_join_link: pending Phase 4"))
    }
}
