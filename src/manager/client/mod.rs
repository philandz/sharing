use tonic::transport::Channel;
use tonic::Status;

use crate::pb::service::budget::budget_service_client::BudgetServiceClient;
use crate::pb::service::budget::{
    AddBudgetMemberRequest, BudgetMember, BudgetRole, CheckRoleRequest, GetBudgetAdminRequest,
    GetBudgetRequest, ListBudgetMembersAdminRequest, ListBudgetMembersRequest,
};
use crate::pb::service::category::category_service_client::CategoryServiceClient;
use crate::pb::service::category::GetCategoryRequest;
use crate::pb::service::identity::identity_service_client::IdentityServiceClient;
use crate::pb::service::identity::ListOrgMembersRequest;

/// OrgUserInfo — see budget service. Sharing enriches participant
/// display_name / email with this on every list_participants read
/// because the local `sharing_participants.display_name` column is
/// empty for members added via `assert_member` (the sharing service
/// never had identity gRPC to look up the name).
#[derive(Debug, Clone)]
pub struct OrgUserInfo {
    pub user_id: String,
    pub display_name: String,
    pub email: String,
    pub avatar: String,
}

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
        // The budget service requires x-user-id metadata on every gRPC call.
        // For internal calls (sharing service calling budget), we inject the
        // caller's user_id so the budget service can identify the actor.
        let mut req = tonic::Request::new(CheckRoleRequest {
            user_id: user_id.to_string(),
            budget_id: budget_id.to_string(),
        });
        if let Ok(v) = tonic::metadata::MetadataValue::try_from(user_id) {
            req.metadata_mut().insert("x-user-id", v);
        }
        let resp = self.inner.check_role(req).await?;
        Ok(BudgetRole::try_from(resp.into_inner().role).unwrap_or(BudgetRole::Unspecified))
    }

    /// Fetch the budget's `org_id` so we can call identity.ListOrgMembers.
    /// Caller must be a budget member (budget service checks `x-user-id`).
    pub async fn get_budget_org_id(
        &mut self,
        user_id: &str,
        budget_id: &str,
    ) -> Result<Option<String>, Status> {
        if user_id.starts_with("g_") {
            // Guests don't have an identity user record; org_id lookup
            // would 403. Return None so the caller can skip enrichment.
            return Ok(None);
        }
        let mut req = tonic::Request::new(GetBudgetRequest {
            budget_id: budget_id.to_string(),
        });
        if let Ok(v) = tonic::metadata::MetadataValue::try_from(user_id) {
            req.metadata_mut().insert("x-user-id", v);
        }
        let resp = self.inner.get_budget(req).await?.into_inner();
        Ok(resp.budget.map(|b| b.org_id))
    }

    /// Internal add-member that bypasses the Manager/Owner check (system_actor=true).
    /// Used when accepting a join link: the budget service trusts the call because
    /// the on-behalf-of user already proved they own the link by being its creator.
    pub async fn add_member_as(
        &mut self,
        budget_id: &str,
        on_behalf_of_user_id: &str,
        new_user_id: &str,
        role: BudgetRole,
    ) -> Result<(), Status> {
        // The budget service requires x-user-id metadata on every gRPC call.
        // For internal calls (sharing → budget), we inject the on-behalf-of
        // user_id so the budget service can audit who initiated the add.
        let mut req = tonic::Request::new(AddBudgetMemberRequest {
            budget_id: budget_id.to_string(),
            user_id: new_user_id.to_string(),
            role: role as i32,
            system_actor: true,
        });
        if let Ok(v) = tonic::metadata::MetadataValue::try_from(on_behalf_of_user_id) {
            req.metadata_mut().insert("x-user-id", v);
        }
        // The budget handler reads x-system-actor from gRPC metadata (not the body).
        if let Ok(v) = tonic::metadata::MetadataValue::try_from("true") {
            req.metadata_mut().insert("x-system-actor", v);
        }
        self.inner.add_budget_member(req).await?;
        Ok(())
    }

    /// List budget members. Used by the backfill CLI to find the parent
    /// budget's owner/manager so the sharing service can ensure their
    /// `sharing_participants` row exists. Caller passes any valid
    /// budget member's user_id as the auth subject — the budget
    /// service permits any authenticated member to read the member list.
    pub async fn list_budget_members(
        &mut self,
        caller_id: &str,
        budget_id: &str,
    ) -> Result<Vec<BudgetMember>, Status> {
        let mut req = tonic::Request::new(ListBudgetMembersRequest {
            budget_id: budget_id.to_string(),
        });
        if let Ok(v) = tonic::metadata::MetadataValue::try_from(caller_id) {
            req.metadata_mut().insert("x-user-id", v);
        }
        let resp = self.inner.list_budget_members(req).await?.into_inner();
        Ok(resp.members)
    }

    /// List all budget members (admin path, requires super_admin JWT).
    /// Used by the backfill CLI which has no member JWT.
    pub async fn list_budget_members_admin(
        &mut self,
        bearer: &str,
        budget_id: &str,
    ) -> Result<Vec<BudgetMember>, Status> {
        let mut req = tonic::Request::new(ListBudgetMembersAdminRequest {
            budget_id: budget_id.to_string(),
        });
        if let Ok(v) = tonic::metadata::MetadataValue::try_from(bearer) {
            req.metadata_mut().insert("authorization", v);
        }
        let resp = self.inner.list_budget_members_admin(req).await?.into_inner();
        Ok(resp.members)
    }

    /// Fetch budget org_id (admin path, requires super_admin JWT).
    /// Used by the backfill CLI which has no member JWT.
    pub async fn get_budget_org_id_admin(
        &mut self,
        bearer: &str,
        budget_id: &str,
    ) -> Result<Option<String>, Status> {
        let mut req = tonic::Request::new(GetBudgetAdminRequest {
            budget_id: budget_id.to_string(),
        });
        if let Ok(v) = tonic::metadata::MetadataValue::try_from(bearer) {
            req.metadata_mut().insert("authorization", v);
        }
        let resp = self.inner.get_budget_admin(req).await?.into_inner();
        Ok(resp.budget.map(|b| b.org_id))
    }
}

pub struct CategoryClient {
    inner: CategoryServiceClient<Channel>,
}

impl CategoryClient {
    pub async fn connect(url: &str) -> Result<Self, tonic::transport::Error> {
        let channel = Channel::from_shared(url.to_string())
            .expect("invalid category gRPC URL")
            .connect()
            .await?;
        Ok(Self {
            inner: CategoryServiceClient::new(channel),
        })
    }

    /// Validate that a category belongs to the given budget. Returns
    /// `INVALID_ARGUMENT` if the category does not exist or belongs to
    /// a different budget. Used by `SharingBiz::add_expense` to reject
    /// mismatched category_ids at the API boundary.
    pub async fn validate_category_in_budget(
        &mut self,
        budget_id: &str,
        category_id: &str,
    ) -> Result<(), Status> {
        let req = tonic::Request::new(GetCategoryRequest {
            category_id: category_id.to_string(),
        });
        let resp = self.inner.get_category(req).await?.into_inner();
        let cat = resp
            .category
            .ok_or_else(|| Status::invalid_argument(format!("category {category_id} not found")))?;
        if cat.budget_id != budget_id {
            return Err(Status::invalid_argument(format!(
                "category {category_id} belongs to budget {} not {budget_id}",
                cat.budget_id
            )));
        }
        Ok(())
    }
}

/// Identity gRPC client for the sharing service. Used to enrich
/// `sharing_participants.display_name` / `.email` / `.avatar` from
/// identity's `users` table — the sharing service has no local copy.
pub struct IdentityClient {
    inner: IdentityServiceClient<Channel>,
}

impl IdentityClient {
    pub async fn connect(url: &str) -> Result<Self, tonic::transport::Error> {
        let channel = Channel::from_shared(url.to_string())
            .expect("invalid identity gRPC URL")
            .connect()
            .await?;
        Ok(Self {
            inner: IdentityServiceClient::new(channel),
        })
    }

    /// Fetch every member of `org_id` from identity service. See the
    /// matching `list_org_users` in the budget service for context.
    pub async fn list_org_users(
        &mut self,
        bearer: &str,
        org_id: &str,
    ) -> Result<Vec<OrgUserInfo>, Status> {
        let mut req = tonic::Request::new(ListOrgMembersRequest {
            org_id: org_id.to_string(),
        });
        // Forward the gateway's bearer verbatim — identity's
        // extract_bearer_token does its own `strip_prefix("Bearer ")`.
        let value = tonic::metadata::MetadataValue::try_from(bearer)
            .map_err(|_| Status::unauthenticated("invalid bearer"))?;
        req.metadata_mut().insert("authorization", value);
        let resp = self.inner.list_org_members(req).await?.into_inner();
        Ok(resp
            .members
            .into_iter()
            .map(|m| OrgUserInfo {
                user_id: m.user_id,
                display_name: m.display_name,
                email: m.email,
                avatar: m.avatar,
            })
            .collect())
    }
}
