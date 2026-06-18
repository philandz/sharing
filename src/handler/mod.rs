use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::manager::biz::SharingBiz;
use crate::manager::validate;
use crate::pb::service::sharing::{
    sharing_service_server::SharingService, AcceptJoinLinkRequest, AcceptJoinLinkResponse,
    AddExpenseRequest, CalculateSettlementRequest, DeleteExpenseRequest, DeleteExpenseResponse,
    DeletePaymentRequest, DeletePaymentResponse, Expense, GenerateJoinLinkRequest,
    GetBalancesRequest, GetBalancesResponse, GetExpenseRequest, GetExpenseResponse, JoinLink,
    ListExpensesRequest, ListExpensesResponse, ListPaymentsRequest, ListPaymentsResponse,
    MarkPaymentRequest, Settlement, SettlementPayment, SplitMethod,
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
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        validate::non_empty("budget_id", &req.budget_id)?;
        validate::non_empty("paid_by", &req.paid_by)?;
        validate::positive_amount(req.total_amount)?;
        validate::non_empty("expense_date", &req.expense_date)?;

        let split_method = SplitMethod::try_from(req.split_method).unwrap_or(SplitMethod::Equal);

        // Build legs: for equal split, divide evenly among all participants.
        // For weighted, pass the weights through to the biz layer which computes
        // the per-user share. For custom (and as a fallback), use the explicit
        // amount on each leg.
        let legs: Vec<(String, i64)> = if split_method == SplitMethod::Equal && !req.legs.is_empty()
        {
            let n = req.legs.len() as i64;
            let per_person = req.total_amount / n;
            let remainder = req.total_amount % n;
            req.legs
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let extra = if (i as i64) < remainder { 1 } else { 0 };
                    (l.user_id.clone(), per_person + extra)
                })
                .collect()
        } else if split_method == SplitMethod::Weighted && !req.legs.is_empty()
        {
            // For weighted splits, the biz layer converts weights to amounts.
            // We pass (user_id, weight) and the biz handles the conversion.
            req.legs
                .iter()
                .map(|l| (l.user_id.clone(), l.weight))
                .collect()
        } else {
            // Custom split (or no legs in the request): use the explicit amount.
            req.legs
                .iter()
                .map(|l| (l.user_id.clone(), l.amount))
                .collect()
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
            )
            .await?;
        Ok(Response::new(expense))
    }

    async fn get_expense(
        &self,
        request: Request<GetExpenseRequest>,
    ) -> Result<Response<GetExpenseResponse>, Status> {
        let user_id = validate::user_id_from_metadata(request.metadata())?;
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
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        let expenses = self.biz.list_expenses(&user_id, &req.budget_id).await?;
        Ok(Response::new(ListExpensesResponse { expenses }))
    }

    async fn delete_expense(
        &self,
        request: Request<DeleteExpenseRequest>,
    ) -> Result<Response<DeleteExpenseResponse>, Status> {
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        self.biz.delete_expense(&user_id, &req.expense_id).await?;
        Ok(Response::new(DeleteExpenseResponse { success: true }))
    }

    async fn calculate_settlement(
        &self,
        request: Request<CalculateSettlementRequest>,
    ) -> Result<Response<Settlement>, Status> {
        let user_id = validate::user_id_from_metadata(request.metadata())?;
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
        let user_id = validate::user_id_from_metadata(request.metadata())?;
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
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        validate::non_empty("token", &req.token)?;
        let resp = self.biz.accept_join_link(&req.token, &user_id).await?;
        Ok(Response::new(resp))
    }

    async fn get_balances(
        &self,
        request: Request<GetBalancesRequest>,
    ) -> Result<Response<GetBalancesResponse>, Status> {
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        let balances = self.biz.get_balances(&user_id, &req.budget_id).await?;
        Ok(Response::new(GetBalancesResponse { balances }))
    }

    async fn mark_payment(
        &self,
        request: Request<MarkPaymentRequest>,
    ) -> Result<Response<SettlementPayment>, Status> {
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        let note = if req.note.is_empty() {
            None
        } else {
            Some(req.note.as_str())
        };
        let payment = self
            .biz
            .mark_payment(
                &user_id,
                &req.budget_id,
                &req.from_user_id,
                &req.to_user_id,
                req.amount,
                &req.paid_at,
                note,
            )
            .await?;
        Ok(Response::new(payment))
    }

    async fn list_payments(
        &self,
        request: Request<ListPaymentsRequest>,
    ) -> Result<Response<ListPaymentsResponse>, Status> {
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        let payments = self.biz.list_payments(&user_id, &req.budget_id).await?;
        Ok(Response::new(ListPaymentsResponse { payments }))
    }

    async fn delete_payment(
        &self,
        request: Request<DeletePaymentRequest>,
    ) -> Result<Response<DeletePaymentResponse>, Status> {
        let user_id = validate::user_id_from_metadata(request.metadata())?;
        let req = request.into_inner();
        self.biz.delete_payment(&user_id, &req.payment_id).await?;
        Ok(Response::new(DeletePaymentResponse {}))
    }
}
