//! Archive guard coverage for the sharing service.
//!
//! An archived budget must reject **every** write, not only the expense
//! writes. The budget service is replaced by an in-process mock that
//! always reports the budget as `archived`; the sharing repository is the
//! real one, so the test needs a database (`DATABASE_URL`) — like the
//! other DB-backed tests in this folder it is `#[ignore]`d by default.
//!
//! Run with: `DATABASE_URL=... cargo test --test archive_guard_test -- --ignored`

use sharing::manager::biz::SharingBiz;
use sharing::manager::client::{BudgetClient, IdentityClient};
use sharing::manager::repository::SharingRepository;
use sharing::pb::service::budget::budget_service_server::{BudgetService, BudgetServiceServer};
use sharing::pb::service::budget::*;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Code, Request, Response, Status};

/// Budget service mock: every budget is `archived`.
struct ArchivedBudgetService;

/// The mock only implements the status lookup — every other RPC is
/// unused by these tests.
#[tonic::async_trait]
impl BudgetService for ArchivedBudgetService {
    async fn get_budget_status(
        &self,
        _request: Request<GetBudgetStatusRequest>,
    ) -> Result<Response<GetBudgetStatusResponse>, Status> {
        Ok(Response::new(GetBudgetStatusResponse {
            status: "archived".to_string(),
            budget_type: 0,
        }))
    }

    async fn create_budget(
        &self,
        _request: Request<CreateBudgetRequest>,
    ) -> Result<Response<CreateBudgetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn get_budget(
        &self,
        _request: Request<GetBudgetRequest>,
    ) -> Result<Response<GetBudgetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn update_budget(
        &self,
        _request: Request<UpdateBudgetRequest>,
    ) -> Result<Response<UpdateBudgetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn delete_budget(
        &self,
        _request: Request<DeleteBudgetRequest>,
    ) -> Result<Response<DeleteBudgetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn list_budgets(
        &self,
        _request: Request<ListBudgetsRequest>,
    ) -> Result<Response<ListBudgetsResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn list_budgets_admin(
        &self,
        _request: Request<ListBudgetsAdminRequest>,
    ) -> Result<Response<ListBudgetsAdminResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn get_budget_admin(
        &self,
        _request: Request<GetBudgetAdminRequest>,
    ) -> Result<Response<GetBudgetAdminResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn list_budget_members_admin(
        &self,
        _request: Request<ListBudgetMembersAdminRequest>,
    ) -> Result<Response<ListBudgetMembersAdminResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn force_close_budget(
        &self,
        _request: Request<ForceCloseBudgetRequest>,
    ) -> Result<Response<ForceCloseBudgetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn set_budget_status(
        &self,
        _request: Request<SetBudgetStatusRequest>,
    ) -> Result<Response<SetBudgetStatusResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn check_role(
        &self,
        _request: Request<CheckRoleRequest>,
    ) -> Result<Response<CheckRoleResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn add_budget_member(
        &self,
        _request: Request<AddBudgetMemberRequest>,
    ) -> Result<Response<AddBudgetMemberResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn update_budget_member_role(
        &self,
        _request: Request<UpdateBudgetMemberRoleRequest>,
    ) -> Result<Response<UpdateBudgetMemberRoleResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn remove_budget_member(
        &self,
        _request: Request<RemoveBudgetMemberRequest>,
    ) -> Result<Response<RemoveBudgetMemberResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn list_budget_members(
        &self,
        _request: Request<ListBudgetMembersRequest>,
    ) -> Result<Response<ListBudgetMembersResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn set_envelope_limit(
        &self,
        _request: Request<SetEnvelopeLimitRequest>,
    ) -> Result<Response<SetEnvelopeLimitResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn get_burn_rate(
        &self,
        _request: Request<GetBurnRateRequest>,
    ) -> Result<Response<GetBurnRateResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn set_rollover_policy(
        &self,
        _request: Request<SetRolloverPolicyRequest>,
    ) -> Result<Response<SetRolloverPolicyResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn get_rollover_policy(
        &self,
        _request: Request<GetRolloverPolicyRequest>,
    ) -> Result<Response<GetRolloverPolicyResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn list_templates(
        &self,
        _request: Request<ListTemplatesRequest>,
    ) -> Result<Response<ListTemplatesResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn create_invest_asset(
        &self,
        _request: Request<CreateInvestAssetRequest>,
    ) -> Result<Response<InvestAsset>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn update_invest_asset(
        &self,
        _request: Request<UpdateInvestAssetRequest>,
    ) -> Result<Response<InvestAsset>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn delete_invest_asset(
        &self,
        _request: Request<DeleteInvestAssetRequest>,
    ) -> Result<Response<DeleteInvestAssetResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn list_invest_assets(
        &self,
        _request: Request<ListInvestAssetsRequest>,
    ) -> Result<Response<ListInvestAssetsResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn get_invest_portfolio_summary(
        &self,
        _request: Request<GetInvestPortfolioSummaryRequest>,
    ) -> Result<Response<InvestPortfolioSummary>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn refresh_portfolio(
        &self,
        _request: Request<RefreshPortfolioRequest>,
    ) -> Result<Response<RefreshPortfolioResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn add_price_snapshot(
        &self,
        _request: Request<AddPriceSnapshotRequest>,
    ) -> Result<Response<PriceSnapshot>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn get_latest_price_snapshot(
        &self,
        _request: Request<GetLatestPriceSnapshotRequest>,
    ) -> Result<Response<PriceSnapshot>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }

    async fn list_price_snapshots(
        &self,
        _request: Request<ListPriceSnapshotsRequest>,
    ) -> Result<Response<ListPriceSnapshotsResponse>, Status> {
        Err(Status::unimplemented("not used by this test"))
    }
}

/// Build a sharing service whose budget service always reports the
/// budget as archived.
async fn sharing_biz_with_archived_budgets() -> SharingBiz {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let repo = SharingRepository::new(&database_url)
        .await
        .expect("sharing repository");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock budget service");
    let addr = listener.local_addr().expect("mock budget service addr");
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(BudgetServiceServer::new(ArchivedBudgetService))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("mock budget service");
    });

    let url = format!("http://{addr}");
    let budget_client = BudgetClient::connect(&url).await.expect("budget client");
    let identity_client = IdentityClient::connect(&url)
        .await
        .expect("identity client");
    SharingBiz::new(repo, budget_client, None, identity_client)
}

/// An archived budget must reject a join through a join link.
#[tokio::test]
#[ignore = "needs DATABASE_URL"]
async fn accept_join_link_rejects_archived_budget() {
    let biz = sharing_biz_with_archived_budgets().await;

    let budget_id = "budget-archived-accept-join";
    let (_id, token, _expires_at) = biz
        .repo
        .create_join_link(budget_id, "user-owner", "org-1")
        .await
        .expect("create join link");

    let err = biz
        .accept_join_link(&token, "user-joiner")
        .await
        .expect_err("an archived budget must reject the join");
    assert_eq!(err.code(), Code::FailedPrecondition);
}

/// An archived budget must reject a member leaving the budget.
#[tokio::test]
#[ignore = "needs DATABASE_URL"]
async fn leave_budget_rejects_archived_budget() {
    let biz = sharing_biz_with_archived_budgets().await;

    let err = biz
        .leave_budget("user-member", "budget-archived-leave", false)
        .await
        .expect_err("an archived budget must reject the leave");
    assert_eq!(err.code(), Code::FailedPrecondition);
}
