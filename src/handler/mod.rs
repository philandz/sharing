use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::manager::biz::SharingBiz;
use crate::manager::validate;
use crate::pb::service::sharing::{
    sharing_service_server::SharingService, AcceptJoinLinkRequest, AcceptJoinLinkResponse,
    AddCommentRequest, AddCommentResponse, AddExpenseRequest, CalculateSettlementRequest,
    DeleteCommentRequest, DeleteCommentResponse, DeleteExpenseRequest, DeleteExpenseResponse,
    DeleteSettlementRequest, DeleteSettlementResponse, Expense, GenerateJoinLinkRequest,
    GetBalancesRequest, GetBalancesResponse, GetExpenseRequest, GetExpenseResponse,
    JoinAsGuestRequest, JoinAsGuestResponse, JoinLink, ListActivityRequest, ListActivityResponse,
    ListCommentsRequest, ListCommentsResponse, ListExpensesRequest, ListExpensesResponse,
    ListParticipantsRequest, ListParticipantsResponse, ListSettlementsRequest,
    ListSettlementsResponse, MarkSettledRequest, PreviewJoinLinkRequest, PreviewJoinLinkResponse,
    RevokeParticipantRequest, RevokeParticipantResponse, Settlement, SettlementConfirmation,
    SplitMethod,
};

pub struct SharingHandler {
    biz: Arc<SharingBiz>,
}

impl SharingHandler {
    pub fn new(biz: Arc<SharingBiz>) -> Self {
        Self { biz }
    }
}

#[tonic::async_trait]
impl SharingService for SharingHandler {
    async fn add_expense(
        &self,
        request: Request<AddExpenseRequest>,
    ) -> Result<Response<Expense>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        validate::non_empty("budget_id", &req.budget_id)?;
        validate::non_empty("paid_by", &req.paid_by)?;
        validate::positive_amount(req.total_amount)?;
        validate::non_empty("expense_date", &req.expense_date)?;

        let split_method = SplitMethod::try_from(req.split_method).unwrap_or(SplitMethod::Equal);

        // Build legs: (user_id, amount_or_share, weight).
        // For equal split, the biz layer divides total/n and absorbs the
        // rounding remainder — we just pass the user_ids with amount=0.
        // For weighted split, amount is 0 (biz computes it); weight is preserved.
        // For custom split, amount is explicit; weight is 0.
        // For percentage, the third slot carries the basis-point value.
        let legs: Vec<(String, i64, i64)> =
            if split_method == SplitMethod::Equal && !req.legs.is_empty() {
                req.legs.iter().map(|l| (l.user_id.clone(), 0, 0)).collect()
            } else if split_method == SplitMethod::Weighted && !req.legs.is_empty() {
                req.legs
                    .iter()
                    .map(|l| (l.user_id.clone(), 0, l.weight))
                    .collect()
            } else if split_method == SplitMethod::Percentage && !req.legs.is_empty() {
                req.legs
                    .iter()
                    .map(|l| (l.user_id.clone(), 0, l.weight))
                    .collect()
            } else {
                req.legs
                    .iter()
                    .map(|l| (l.user_id.clone(), l.amount, 0))
                    .collect()
            };

        // For BY_ITEM, pass the items through. The biz layer converts them
        // into per-user totals.
        let items: Vec<crate::manager::biz::ByItemInput> = if split_method == SplitMethod::ByItem {
            req.items
                .iter()
                .map(|it| crate::manager::biz::ByItemInput {
                    label: it.label.clone(),
                    amount: it.amount,
                    assignments: it
                        .assignments
                        .iter()
                        .map(|a| crate::manager::biz::ItemAssignmentInput {
                            user_id: a.user_id.clone(),
                            numerator: a.numerator,
                        })
                        .collect(),
                })
                .collect()
        } else {
            vec![]
        };

        let cat_id = if req.category_id.is_empty() {
            None
        } else {
            Some(req.category_id.as_str())
        };

        let expense = self
            .biz
            .add_expense(
                &user_id,
                &req.budget_id,
                &req.paid_by,
                req.total_amount,
                &req.description,
                &req.expense_date,
                cat_id,
                split_method,
                legs,
                items,
            )
            .await?;
        Ok(Response::new(expense))
    }

    async fn get_expense(
        &self,
        request: Request<GetExpenseRequest>,
    ) -> Result<Response<GetExpenseResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let expense = self.biz.get_expense(&user_id, &req.expense_id).await?;
        Ok(Response::new(GetExpenseResponse {
            expense: Some(expense),
        }))
    }

    async fn list_expenses(
        &self,
        request: Request<ListExpensesRequest>,
    ) -> Result<Response<ListExpensesResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let expenses = self.biz.list_expenses(&user_id, &req.budget_id).await?;
        Ok(Response::new(ListExpensesResponse { expenses }))
    }

    async fn delete_expense(
        &self,
        request: Request<DeleteExpenseRequest>,
    ) -> Result<Response<DeleteExpenseResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        self.biz.delete_expense(&user_id, &req.expense_id).await?;
        Ok(Response::new(DeleteExpenseResponse { success: true }))
    }

    async fn calculate_settlement(
        &self,
        request: Request<CalculateSettlementRequest>,
    ) -> Result<Response<Settlement>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let settlement = self
            .biz
            .calculate_settlement(&user_id, &req.budget_id)
            .await?;
        Ok(Response::new(settlement))
    }

    async fn generate_join_link(
        &self,
        request: Request<GenerateJoinLinkRequest>,
    ) -> Result<Response<JoinLink>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let link = self
            .biz
            .generate_join_link(&user_id, &req.budget_id)
            .await?;
        Ok(Response::new(link))
    }

    async fn accept_join_link(
        &self,
        request: Request<AcceptJoinLinkRequest>,
    ) -> Result<Response<AcceptJoinLinkResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        validate::non_empty("token", &req.token)?;
        let resp = self.biz.accept_join_link(&req.token, &user_id).await?;
        Ok(Response::new(resp))
    }

    async fn get_balances(
        &self,
        request: Request<GetBalancesRequest>,
    ) -> Result<Response<GetBalancesResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let balances = self.biz.get_balances(&user_id, &req.budget_id).await?;
        Ok(Response::new(GetBalancesResponse { balances }))
    }

    async fn mark_settled(
        &self,
        request: Request<MarkSettledRequest>,
    ) -> Result<Response<SettlementConfirmation>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let note = if req.note.is_empty() {
            None
        } else {
            Some(req.note.as_str())
        };
        let confirmation = self
            .biz
            .mark_settled(
                &user_id,
                &req.budget_id,
                &req.from_participant_id,
                &req.to_participant_id,
                req.amount,
                &req.settled_at,
                note,
            )
            .await?;
        Ok(Response::new(confirmation))
    }

    async fn list_settlements(
        &self,
        request: Request<ListSettlementsRequest>,
    ) -> Result<Response<ListSettlementsResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let confirmations = self.biz.list_payments(&user_id, &req.budget_id).await?;
        Ok(Response::new(ListSettlementsResponse { confirmations }))
    }

    async fn delete_settlement(
        &self,
        request: Request<DeleteSettlementRequest>,
    ) -> Result<Response<DeleteSettlementResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        self.biz
            .delete_payment(&user_id, &req.confirmation_id)
            .await?;
        Ok(Response::new(DeleteSettlementResponse { success: true }))
    }

    // -----------------------------------------------------------------------
    // Per-expense comments
    // -----------------------------------------------------------------------

    async fn add_comment(
        &self,
        request: Request<AddCommentRequest>,
    ) -> Result<Response<AddCommentResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let comment = self
            .biz
            .add_comment(&user_id, &req.expense_id, &req.body)
            .await?;
        Ok(Response::new(AddCommentResponse {
            comment: Some(comment),
        }))
    }

    async fn list_comments(
        &self,
        request: Request<ListCommentsRequest>,
    ) -> Result<Response<ListCommentsResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let comments = self.biz.list_comments(&user_id, &req.expense_id).await?;
        Ok(Response::new(ListCommentsResponse { comments }))
    }

    async fn delete_comment(
        &self,
        request: Request<DeleteCommentRequest>,
    ) -> Result<Response<DeleteCommentResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        self.biz.delete_comment(&user_id, &req.comment_id).await?;
        Ok(Response::new(DeleteCommentResponse { success: true }))
    }

    // -----------------------------------------------------------------------
    // Activity log
    // -----------------------------------------------------------------------

    async fn list_activity(
        &self,
        request: Request<ListActivityRequest>,
    ) -> Result<Response<ListActivityResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let entries = self
            .biz
            .list_activity(&user_id, &req.budget_id, req.since_unix, req.limit)
            .await?;
        Ok(Response::new(ListActivityResponse { entries }))
    }

    // -----------------------------------------------------------------------
    // Participants
    // -----------------------------------------------------------------------

    async fn list_participants(
        &self,
        request: Request<ListParticipantsRequest>,
    ) -> Result<Response<ListParticipantsResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        // Self-heal: ensure the caller has a sharing_participants row
        // so the members card includes them on first read, not just
        // after a write.
        self.biz
            .ensure_member_participant_row(&req.budget_id, &user_id)
            .await;
        let participants = self.biz.list_participants_typed(&req.budget_id).await?;
        Ok(Response::new(ListParticipantsResponse { participants }))
    }

    async fn revoke_participant(
        &self,
        request: Request<RevokeParticipantRequest>,
    ) -> Result<Response<RevokeParticipantResponse>, Status> {
        let user_id = self
            .biz
            .participant_id_from_metadata(request.metadata())
            .await?;
        let req = request.into_inner();
        let ok = self
            .biz
            .revoke_participant(&user_id, &req.budget_id, &req.participant_id)
            .await?;
        Ok(Response::new(RevokeParticipantResponse { success: ok }))
    }

    // -----------------------------------------------------------------------
    // Join link (account-free guest path)
    // -----------------------------------------------------------------------

    async fn preview_join_link(
        &self,
        request: Request<PreviewJoinLinkRequest>,
    ) -> Result<Response<PreviewJoinLinkResponse>, Status> {
        // Public entry point — guests call this before they have any
        // session token, so do NOT require x-user-id / x-session-token.
        let _ = request.metadata();
        let req = request.into_inner();
        let preview = self.biz.preview_join_link(&req.token).await?;
        Ok(Response::new(preview))
    }

    async fn join_as_guest(
        &self,
        request: Request<JoinAsGuestRequest>,
    ) -> Result<Response<JoinAsGuestResponse>, Status> {
        // Same as preview — guests join here from the public link.
        let _ = request.metadata();
        let req = request.into_inner();
        let (session_token, display_name, participant_id, budget_id, kind) = self
            .biz
            .join_as_guest(&req.token, &req.display_name)
            .await?;
        Ok(Response::new(JoinAsGuestResponse {
            session_token,
            participant_id,
            budget_id,
            display_name,
            kind,
        }))
    }
}

// Helper to map a sharing error to a tonic Status for the handler body.
#[allow(dead_code)]
impl SharingHandler {
    fn internal(e: impl ToString) -> Status {
        Status::internal(e.to_string())
    }
}
