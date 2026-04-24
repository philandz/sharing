use tonic::transport::Channel;
use tonic::Status;

use crate::pb::service::budget::budget_service_client::BudgetServiceClient;
use crate::pb::service::budget::{BudgetRole, CheckRoleRequest};

pub struct BudgetClient {
    inner: BudgetServiceClient<Channel>,
}

impl BudgetClient {
    pub async fn connect(url: &str) -> Result<Self, tonic::transport::Error> {
        let channel = Channel::from_shared(url.to_string())
            .expect("invalid budget gRPC URL")
            .connect()
            .await?;
        Ok(Self {
            inner: BudgetServiceClient::new(channel),
        })
    }

    pub async fn check_role(
        &mut self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<BudgetRole, Status> {
        let resp = self
            .inner
            .check_role(tonic::Request::new(CheckRoleRequest {
                user_id: user_id.to_string(),
                budget_id: budget_id.to_string(),
            }))
            .await?;
        Ok(BudgetRole::try_from(resp.into_inner().role).unwrap_or(BudgetRole::Unspecified))
    }
}
