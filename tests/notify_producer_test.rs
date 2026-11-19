//! Sharing notification producers against a real MySQL: expense / settlement / comment events
//! land in `sharing_notify_outbox` for members only (never guests, never the actor).
//! `DATABASE_URL=... cargo test --test notify_producer_test -- --ignored`

use philand_event::MySqlOutbox;
use sharing::manager::biz::SharingBiz;
use sharing::manager::client::{BudgetClient, IdentityClient};
use sharing::manager::notifier::Notifier;
use sharing::manager::repository::SharingRepository;
use sharing::pb::service::budget::budget_service_server::{BudgetService, BudgetServiceServer};
use sharing::pb::service::budget::*;
use sharing::pb::service::sharing::SplitMethod;
use sqlx::{MySqlPool, Row};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};

/// Budget service mock: every budget is active and every caller an Owner.
struct ActiveBudgetService;

/// The mock only implements the status lookup — every other RPC is
/// unused by these tests.
#[tonic::async_trait]
impl BudgetService for ActiveBudgetService {
    async fn get_budget_status(
        &self,
        _request: Request<GetBudgetStatusRequest>,
    ) -> Result<Response<GetBudgetStatusResponse>, Status> {
        Ok(Response::new(GetBudgetStatusResponse {
            status: "active".to_string(),
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
        Ok(Response::new(CheckRoleResponse {
            role: BudgetRole::Owner as i32,
            is_member: true,
            ..Default::default()
        }))
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

async fn setup() -> (SharingBiz, MySqlPool) {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let repo = SharingRepository::new(&url).await.expect("repo");
    let pool = MySqlPool::connect(&url).await.expect("pool");
    let ddl = include_str!("../migrations/20261110000002_notify_outbox.sql");
    sqlx::query(ddl.split(';').find(|s| s.contains("CREATE TABLE")).unwrap())
        .execute(&pool)
        .await
        .expect("outbox table");
    sqlx::query("DELETE FROM sharing_notify_outbox")
        .execute(&pool)
        .await
        .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(BudgetServiceServer::new(ActiveBudgetService))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let url = format!("http://{addr}");
    let budget_client = BudgetClient::connect(&url).await.unwrap();
    let identity_client = IdentityClient::connect(&url).await.unwrap();
    let outbox = MySqlOutbox::new(pool.clone(), "sharing_notify_outbox").unwrap();
    let biz = SharingBiz::new(repo, budget_client, None, identity_client)
        .with_notifier(Notifier::new(outbox, pool.clone()));
    (biz, pool)
}

async fn events(pool: &MySqlPool, kind: &str) -> Vec<serde_json::Value> {
    sqlx::query("SELECT payload_json FROM sharing_notify_outbox WHERE event_type = ?")
        .bind(kind)
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| serde_json::from_str(&r.get::<String, _>("payload_json")).unwrap())
        .collect()
}

fn recipients(ev: &serde_json::Value) -> Vec<String> {
    let mut v: Vec<String> = ev["recipient_user_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

#[tokio::test]
#[ignore = "needs DATABASE_URL"]
async fn expense_settlement_comment_notify_members_only() {
    let (biz, pool) = setup().await;
    let budget = uuid::Uuid::new_v4().to_string();

    // u_actor adds a 300 expense paid by m2, split with a guest and m3.
    let legs = vec![
        ("m2".to_string(), 100, 0),
        ("g_guest".to_string(), 100, 0),
        ("u_m3".to_string(), 100, 0),
    ];
    let exp = biz
        .add_expense(
            "actor",
            &budget,
            "m2",
            300,
            "dinner",
            "2026-11-10",
            None,
            SplitMethod::Equal,
            legs,
            vec![],
        )
        .await
        .expect("add_expense");
    let evs = events(&pool, "sharing.expense_added").await;
    assert_eq!(evs.len(), 1);
    assert_eq!(recipients(&evs[0]), vec!["m2", "m3"]);

    // Comment by m2 reaches the creator (actor) and m3, not m2 or the guest.
    biz.add_comment("m2", &exp.base.as_ref().unwrap().id, "looks right")
        .await
        .expect("comment");
    let evs = events(&pool, "sharing.comment_added").await;
    assert_eq!(evs.len(), 1);
    assert_eq!(recipients(&evs[0]), vec!["actor", "m3"]);

    // Settlement m3 -> m2 marked by m3 only notifies m2.
    biz.mark_settled("m3", &budget, "m3", "m2", 50, "2026-11-10", None)
        .await
        .expect("settle");
    let evs = events(&pool, "sharing.settlement_marked").await;
    assert_eq!(evs.len(), 1);
    assert_eq!(recipients(&evs[0]), vec!["m2"]);
}
