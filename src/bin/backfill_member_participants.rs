//! Backfill sharing_participants rows for budget owners whose row
//! was never inserted by the (now-fixed) lazy-upsert path. Idempotent
//! and safe to re-run; in dry-run mode, logs the would-be inserts
//! without writing.
//!
//! Usage:
//!   cargo run --bin backfill_member_participants --release -- \
//!     --budget-id <id>          # scope to one budget
//!     --dry-run                 # log would-be inserts only
//!     --limit <n>               # cap the number of budgets processed
//!     --database-url <url>      # default: $DATABASE_URL
//!     --budget-grpc <url>       # default: $BUDGET_GRPC_URL or http://127.0.0.1:50103
//!     --verbose
//!
//! Algorithm:
//!   1. Enumerate distinct budget_ids with sharing activity.
//!   2. For each, pick any active member from sharing_participants as
//!      the auth subject (budget service requires the caller to be a
//!      member of the budget), then list budget members and pick
//!      the Owner (or Manager) as the missing row.
//!   3. Check sharing_participants for an existing active member row;
//!      insert if absent.
//!   4. Log progress every 100 budgets.

use std::time::Instant;

use sharing::manager::client::BudgetClient;
use sharing::manager::repository::SharingRepository;
use sharing::pb::service::budget::BudgetRole;

#[derive(Debug, Default)]
struct Args {
    budget_id: Option<String>,
    dry_run: bool,
    limit: Option<usize>,
    database_url: Option<String>,
    budget_grpc: Option<String>,
    bearer: Option<String>,
    verbose: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(a) = iter.next() {
        let mut next = || iter.next().ok_or_else(|| format!("missing value for {a}"));
        match a.as_str() {
            "--budget-id" => args.budget_id = Some(next()?),
            "--dry-run" => args.dry_run = true,
            "--limit" => {
                args.limit = Some(next()?.parse().map_err(|e| format!("bad --limit: {e}"))?);
            }
            "--database-url" => args.database_url = Some(next()?),
            "--budget-grpc" => args.budget_grpc = Some(next()?),
            "--bearer" => args.bearer = Some(next()?),
            "--verbose" => args.verbose = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(args)
}

fn print_help() {
    eprintln!("backfill_member_participants — see source for flags");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    philand_logging::init(
        "backfill_member_participants",
        Some("sharing=info,backfill=info"),
    );

    let args = parse_args().map_err(anyhow::Error::msg)?;
    if args.verbose {
        eprintln!("args = {:?}", args);
    }

    let database_url = args
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL not set (use --database-url or env)"))?;
    let budget_grpc = args
        .budget_grpc
        .or_else(|| std::env::var("BUDGET_GRPC_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:50103".to_string());

    let repo = SharingRepository::new(&database_url).await?;
    let mut budget_client = BudgetClient::connect(&budget_grpc).await?;

    let bearer = args
        .bearer
        .or_else(|| std::env::var("BACKFILL_BEARER").ok())
        .ok_or_else(|| anyhow::anyhow!("BACKFILL_BEARER not set (use --bearer or env)"))?;

    let budget_ids: Vec<String> = if let Some(id) = args.budget_id {
        vec![id]
    } else {
        repo.distinct_budget_ids_with_activity().await?
    };
    let budget_ids: Vec<String> = match args.limit {
        Some(n) => budget_ids.into_iter().take(n).collect(),
        None => budget_ids,
    };

    tracing::info!(
        count = budget_ids.len(),
        dry_run = args.dry_run,
        "starting backfill"
    );

    let started = Instant::now();
    let mut backfilled: u64 = 0;
    let mut skipped: u64 = 0;
    let mut errors: u64 = 0;
    let mut last_progress = 0usize;

    for budget_id in &budget_ids {
        let budget_id = budget_id.as_str();
        match backfill_one(&repo, &mut budget_client, &bearer, budget_id, args.dry_run).await {
            BackfillOutcome::Inserted => backfilled += 1,
            BackfillOutcome::Skipped => skipped += 1,
            BackfillOutcome::Failed => errors += 1,
        }
        let processed = (backfilled + skipped + errors) as usize;
        if processed - last_progress >= 100 {
            last_progress = processed;
            tracing::info!(
                backfilled,
                skipped,
                errors,
                elapsed_s = started.elapsed().as_secs(),
                "progress"
            );
        }
    }

    tracing::info!(
        backfilled,
        skipped,
        errors,
        elapsed_s = started.elapsed().as_secs(),
        dry_run = args.dry_run,
        "done"
    );

    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

enum BackfillOutcome {
    Inserted,
    Skipped,
    Failed,
}

async fn backfill_one(
    repo: &SharingRepository,
    budget_client: &mut BudgetClient,
    bearer: &str,
    budget_id: &str,
    dry_run: bool,
) -> BackfillOutcome {
    // Use super_admin JWT to list all budget members and fetch org_id
    // without needing a member JWT.
    let members = match budget_client
        .list_budget_members_admin(bearer, budget_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = ?e, budget_id, "list_budget_members_admin failed");
            return BackfillOutcome::Failed;
        }
    };

    // Pick the first Owner or Manager. Use a stable order so re-runs
    // are deterministic if multiple owners exist (the proto allows it
    // for shared custody scenarios).
    let owners: Vec<_> = members
        .iter()
        .filter(|m| {
            let role = BudgetRole::try_from(m.role).unwrap_or(BudgetRole::Unspecified);
            matches!(role, BudgetRole::Owner | BudgetRole::Manager)
        })
        .collect();

    if owners.is_empty() {
        tracing::warn!(budget_id, "no Owner/Manager found; skipping");
        return BackfillOutcome::Skipped;
    }

    // Fetch org_id so the participant row can be enriched by guests later.
    let org_id = match budget_client.get_budget_org_id_admin(bearer, budget_id).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            tracing::warn!(budget_id, "budget has no org_id; skipping");
            return BackfillOutcome::Skipped;
        }
        Err(e) => {
            tracing::warn!(error = ?e, budget_id, "get_budget_org_id_admin failed; skipping");
            return BackfillOutcome::Failed;
        }
    };

    let mut any_inserted = false;
    for m in owners {
        let user_id = m.user_id.clone();
        let display_name = m.display_name.clone();
        match repo.find_member_participant(budget_id, &user_id).await {
            Ok(Some(_)) => {
                if dry_run {
                    tracing::info!(budget_id, user_id, "would skip (already present)");
                }
            }
            Ok(None) => {
                if dry_run {
                    tracing::info!(budget_id, user_id, display_name, "would insert");
                    any_inserted = true;
                    continue;
                }
                match repo
                    .upsert_member_participant(budget_id, &user_id, &display_name, &org_id)
                    .await
                {
                    Ok(_) => {
                        tracing::info!(budget_id, user_id, display_name, "inserted");
                        any_inserted = true;
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, budget_id, user_id, "upsert failed");
                        return BackfillOutcome::Failed;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = ?e, budget_id, user_id, "find_member_participant failed");
                return BackfillOutcome::Failed;
            }
        }
    }

    if any_inserted {
        BackfillOutcome::Inserted
    } else {
        BackfillOutcome::Skipped
    }
}

async fn pick_caller(repo: &SharingRepository, budget_id: &str) -> Option<String> {
    match repo.any_active_user_id(budget_id).await {
        Ok(Some(u)) => Some(u),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = ?e, budget_id, "any_active_user_id failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharing::pb::service::budget::BudgetMember;

    #[test]
    fn parse_args_defaults_to_empty() {
        let a = Args::default();
        assert!(a.budget_id.is_none());
        assert!(!a.dry_run);
        assert!(a.limit.is_none());
    }

    #[test]
    fn owners_filter_picks_owner_role() {
        let members = [
            member("u1", "Alice", BudgetRole::Owner),
            member("u2", "Bob", BudgetRole::Contributor),
            member("u3", "Carol", BudgetRole::Manager),
            member("u4", "Dave", BudgetRole::Viewer),
        ];
        let owners: Vec<String> = members
            .iter()
            .filter(|m| {
                let role = BudgetRole::try_from(m.role).unwrap_or(BudgetRole::Unspecified);
                matches!(role, BudgetRole::Owner | BudgetRole::Manager)
            })
            .map(|m| m.user_id.clone())
            .collect();
        assert_eq!(owners, ["u1", "u3"]);
    }

    fn member(user_id: &str, name: &str, role: BudgetRole) -> BudgetMember {
        BudgetMember {
            budget_id: "b1".into(),
            user_id: user_id.into(),
            display_name: name.into(),
            email: String::new(),
            role: role as i32,
            avatar: String::new(),
        }
    }
}
