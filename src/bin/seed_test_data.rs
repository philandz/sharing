//! Seed test data for browser QA verification of sharing + invest UI surfaces.
//!
//! Creates (if not already present):
//!   - 1 admin user with an org
//!   - 1 sharing budget owned by admin, with 2 members + 1 expense
//!   - 1 invest budget owned by admin, with 1 asset
//!
//! Idempotent: re-running is safe. Skips steps where data already exists.
//!
//! Run with: cargo run --bin seed-test-data

use sqlx::Row;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL env var required"))?;
    let pool = sqlx::MySqlPool::connect(&db_url).await?;
    let now = chrono::Utc::now().timestamp();

    println!("=== Philandz QA Seed Data ===\n");

    // ---- 1. Find or create admin user + org ----
    let (admin_user_id, admin_email, org_id) = find_or_create_admin(&pool, now).await?;

    // ---- 2. Find or create sharing budget ----
    let sharing_budget_id =
        find_or_create_sharing_budget(&pool, &org_id, &admin_user_id, now).await?;

    // ---- 3. Find or create invest budget ----
    let invest_budget_id =
        find_or_create_invest_budget(&pool, &org_id, &admin_user_id, now).await?;

    // ---- 4. Seed sharing participants (2 members) ----
    seed_sharing_participants(&pool, &sharing_budget_id, &admin_user_id, &admin_email, now).await?;

    // ---- 5. Seed a sharing expense ----
    seed_sharing_expense(&pool, &sharing_budget_id, &admin_user_id, now).await?;

    // ---- 6. Seed an invest asset ----
    seed_invest_asset(&pool, &invest_budget_id, &admin_user_id, now).await?;

    // ---- Verify ----
    verify(&pool, &sharing_budget_id, &invest_budget_id).await?;

    println!("\n=== Seed Summary ===");
    println!("admin_email: {}", admin_email);
    println!("admin_user_id: {}", admin_user_id);
    println!("org_id: {}", org_id);
    println!("sharing_budget_id: {}", sharing_budget_id);
    println!("invest_budget_id: {}", invest_budget_id);
    println!("\nDone! Run `make smoke` or open the app to verify UI surfaces.");

    Ok(())
}

async fn find_or_create_admin(
    pool: &sqlx::MySqlPool,
    _now: i64,
) -> anyhow::Result<(String, String, String)> {
    // Try super-admin email first, then any existing user with an org
    let admin_email = "laphi1612@gmail.com";

    // Check if user exists with org
    let row = sqlx::query(
        "SELECT u.id as user_id, om.org_id
         FROM philandz.users u
         JOIN philandz.organization_members om ON BINARY om.user_id = BINARY u.id
         WHERE u.email = ?
         LIMIT 1",
    )
    .bind(admin_email)
    .fetch_optional(pool)
    .await?;

    if let Some(r) = row {
        let user_id: String = r.get("user_id");
        let org_id: String = r.get("org_id");
        println!(
            "[admin] found existing admin: {} / {}",
            admin_email,
            &user_id[..8]
        );
        return Ok((user_id, admin_email.to_string(), org_id));
    }

    // Check if user exists without org
    let user_row = sqlx::query("SELECT id FROM philandz.users WHERE email = ?")
        .bind(admin_email)
        .fetch_optional(pool)
        .await?;

    if let Some(r) = user_row {
        let user_id: String = r.get("id");
        // Create org for existing user
        let org_id = uuid::Uuid::new_v4().to_string();
        let org_name = format!("{}'s Org", admin_email.split('@').next().unwrap_or("admin"));
        let mut tx = pool.begin().await?;
        sqlx::query("INSERT INTO philandz.organizations (id, name, owner_user_id, status) VALUES (?, ?, ?, 'active')")
            .bind(&org_id).bind(&org_name).bind(&user_id)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO philandz.organization_members (org_id, user_id, org_role, status) VALUES (?, ?, 'owner', 'active')")
            .bind(&org_id).bind(&user_id)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        println!(
            "[admin] created org {} for existing user {}",
            &org_id[..8],
            &user_id[..8]
        );
        return Ok((user_id, admin_email.to_string(), org_id));
    }

    // No admin user — use the first user in the DB
    let first_user = sqlx::query(
        "SELECT u.id as user_id, om.org_id
         FROM philandz.users u
         LEFT JOIN philandz.organization_members om ON om.user_id = u.id
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    if let Some(r) = first_user {
        let user_id: String = r.get("user_id");
        let org_id: Option<String> = r.get("org_id");
        if let Some(org_id) = org_id {
            println!(
                "[admin] using first user-with-org: {} / {}",
                &user_id[..8],
                &org_id[..8]
            );
            return Ok((user_id, "existing-user".to_string(), org_id));
        }
        // Create org
        let org_id = uuid::Uuid::new_v4().to_string();
        let mut tx = pool.begin().await?;
        sqlx::query("INSERT INTO philandz.organizations (id, name, owner_user_id, status) VALUES (?, ?, ?, 'active')")
            .bind(&org_id).bind("QA Org").bind(&user_id)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO philandz.organization_members (org_id, user_id, org_role, status) VALUES (?, ?, 'owner', 'active')")
            .bind(&org_id).bind(&user_id)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        println!(
            "[admin] created org {} for existing user {}",
            &org_id[..8],
            &user_id[..8]
        );
        return Ok((user_id, "existing-user".to_string(), org_id));
    }

    anyhow::bail!("No users found in DB. Cannot seed — run bootstrap or create a user first.");
}

async fn find_or_create_sharing_budget(
    pool: &sqlx::MySqlPool,
    org_id: &str,
    owner_user_id: &str,
    now: i64,
) -> anyhow::Result<String> {
    // Check if a sharing budget already exists for this org
    let row = sqlx::query(
        "SELECT id FROM philandz.budgets WHERE org_id = ? AND budget_type = 'sharing' AND deleted_at IS NULL LIMIT 1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await?;

    if let Some(r) = row {
        let id: String = r.get("id");
        println!("[sharing] found existing sharing budget: {}", &id[..8]);
        return Ok(id);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO philandz.budgets (id, org_id, owner_id, name, budget_type, currency, status, created_by, created_at, updated_at)
         VALUES (?, ?, ?, 'QA Sharing Budget', 'sharing', 'VND', 'active', ?, ?, ?)",
    )
    .bind(&id).bind(org_id).bind(owner_user_id).bind(owner_user_id).bind(now).bind(now)
    .execute(&mut *tx).await?;

    // Add owner as budget member
    sqlx::query(
        "INSERT INTO philandz.budget_members (id, budget_id, user_id, role, created_at, updated_at)
         VALUES (?, ?, ?, 'owner', ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&id)
    .bind(owner_user_id)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    println!("[sharing] created sharing budget: {}", &id[..8]);
    Ok(id)
}

async fn find_or_create_invest_budget(
    pool: &sqlx::MySqlPool,
    org_id: &str,
    owner_user_id: &str,
    now: i64,
) -> anyhow::Result<String> {
    let row = sqlx::query(
        "SELECT id FROM philandz.budgets WHERE org_id = ? AND budget_type = 'invest' AND deleted_at IS NULL LIMIT 1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await?;

    if let Some(r) = row {
        let id: String = r.get("id");
        println!("[invest] found existing invest budget: {}", &id[..8]);
        return Ok(id);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO philandz.budgets (id, org_id, owner_id, name, budget_type, currency, status, created_by, created_at, updated_at)
         VALUES (?, ?, ?, 'QA Invest Budget', 'invest', 'VND', 'active', ?, ?, ?)",
    )
    .bind(&id).bind(org_id).bind(owner_user_id).bind(owner_user_id).bind(now).bind(now)
    .execute(&mut *tx).await?;

    sqlx::query(
        "INSERT INTO philandz.budget_members (id, budget_id, user_id, role, created_at, updated_at)
         VALUES (?, ?, ?, 'owner', ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&id)
    .bind(owner_user_id)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    println!("[invest] created invest budget: {}", &id[..8]);
    Ok(id)
}

async fn seed_sharing_participants(
    pool: &sqlx::MySqlPool,
    budget_id: &str,
    admin_user_id: &str,
    admin_email: &str,
    now: i64,
) -> anyhow::Result<()> {
    // Count existing members
    let before: i64 =
        sqlx::query("SELECT COUNT(*) as c FROM sharing_participants WHERE budget_id = ?")
            .bind(budget_id)
            .fetch_one(pool)
            .await?
            .get("c");

    if before >= 2 {
        println!("[sharing] already has {} participants, skipping", before);
        return Ok(());
    }

    // Add admin as member
    let admin_participant_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT IGNORE INTO sharing_participants (id, budget_id, participant_kind, user_id, display_name, joined_at, last_seen_at)
         VALUES (?, ?, 'member', ?, ?, ?, ?)",
    )
    .bind(&admin_participant_id)
    .bind(budget_id)
    .bind(admin_user_id)
    .bind(format!("Admin ({})", admin_email))
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // Find a second user to add as member
    let second_user: Option<(String, String)> = sqlx::query(
        "SELECT u.id, COALESCE(u.display_name, u.email) as name
         FROM philandz.users u
         WHERE u.id != ?
         ORDER BY u.created_at
         LIMIT 1",
    )
    .bind(admin_user_id)
    .fetch_optional(pool)
    .await?
    .map(|r| (r.get::<String, _>("id"), r.get::<String, _>("name")));

    if let Some((uid, name)) = second_user {
        let pid2 = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT IGNORE INTO sharing_participants (id, budget_id, participant_kind, user_id, display_name, joined_at, last_seen_at)
             VALUES (?, ?, 'member', ?, ?, ?, ?)",
        )
        .bind(&pid2).bind(budget_id).bind(&uid).bind(&name).bind(now).bind(now)
        .execute(pool)
        .await?;
        println!("[sharing] added member: {}", name);
    } else {
        println!("[sharing] only admin participant (no second user found)");
    }

    let after: i64 =
        sqlx::query("SELECT COUNT(*) as c FROM sharing_participants WHERE budget_id = ?")
            .bind(budget_id)
            .fetch_one(pool)
            .await?
            .get("c");
    println!("[sharing] participants: {}", after);
    Ok(())
}

async fn seed_sharing_expense(
    pool: &sqlx::MySqlPool,
    budget_id: &str,
    payer_user_id: &str,
    now: i64,
) -> anyhow::Result<()> {
    let existing: i64 =
        sqlx::query("SELECT COUNT(*) as c FROM sharing_expenses WHERE budget_id = ?")
            .bind(budget_id)
            .fetch_one(pool)
            .await?
            .get("c");

    if existing > 0 {
        println!("[sharing] already has {} expenses, skipping", existing);
        return Ok(());
    }

    let expense_id = uuid::Uuid::new_v4().to_string();
    let amount: i64 = 150000; // 150,000 VND
    sqlx::query(
        "INSERT INTO sharing_expenses (id, budget_id, paid_by, total_amount, description, expense_date, split_method, created_by, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'QA Test Expense - Dinner', CURDATE(), 'equal', ?, ?, ?)",
    )
    .bind(&expense_id).bind(budget_id).bind(payer_user_id).bind(amount).bind(payer_user_id).bind(now).bind(now)
    .execute(pool)
    .await?;

    // Get participant user_ids for the legs
    let participants: Vec<String> =
        sqlx::query("SELECT user_id FROM sharing_participants WHERE budget_id = ?")
            .bind(budget_id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| r.get("user_id"))
            .collect();

    let split_amount = amount / participants.len().max(1) as i64;
    for p_user_id in &participants {
        let leg_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO sharing_expense_legs (id, expense_id, user_id, amount, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&leg_id)
        .bind(&expense_id)
        .bind(p_user_id)
        .bind(split_amount)
        .bind(now)
        .execute(pool)
        .await?;
    }

    // Update balances (simplified: payer is owed by others)
    for p_user_id in &participants {
        let bal_id = uuid::Uuid::new_v4().to_string();
        let balance = if p_user_id == payer_user_id {
            amount - split_amount // payer is owed
        } else {
            -split_amount // others owe
        };
        sqlx::query(
            "INSERT INTO sharing_balances (id, budget_id, user_id, net_balance, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON DUPLICATE KEY UPDATE net_balance = net_balance + VALUES(net_balance), updated_at = VALUES(updated_at)",
        )
        .bind(&bal_id).bind(budget_id).bind(p_user_id).bind(balance).bind(now)
        .execute(pool)
        .await?;
    }

    println!(
        "[sharing] created expense: {} ({} VND)",
        &expense_id[..8],
        amount
    );
    Ok(())
}

async fn seed_invest_asset(
    pool: &sqlx::MySqlPool,
    budget_id: &str,
    owner_user_id: &str,
    now: i64,
) -> anyhow::Result<()> {
    let existing: i64 =
        sqlx::query("SELECT COUNT(*) as c FROM portfolio_assets WHERE budget_id = ?")
            .bind(budget_id)
            .fetch_one(pool)
            .await?
            .get("c");

    if existing > 0 {
        println!("[invest] already has {} assets, skipping", existing);
        return Ok(());
    }

    let asset_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO portfolio_assets (id, budget_id, asset_class, display_name, currency, status, opened_on, created_by, created_at, updated_at)
         VALUES (?, ?, 'savings_account', 'QA Savings Account', 'VND', 'active', ?, ?, ?, ?)",
    )
    .bind(&asset_id).bind(budget_id).bind(now).bind(owner_user_id).bind(now).bind(now)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO portfolio_savings_accounts (asset_id, provider, account_reference_masked, current_balance, balance_as_of, annual_rate, interest_method, payout_type, opened_on)
         VALUES (?, 'Test Bank', '****1234', 5000000, ?, '4.5', 'compound', 'on_demand', ?)",
    )
    .bind(&asset_id).bind(now).bind(now)
    .execute(pool)
    .await?;

    println!("[invest] created savings account asset: {}", &asset_id[..8]);
    Ok(())
}

async fn verify(pool: &sqlx::MySqlPool, sharing_id: &str, invest_id: &str) -> anyhow::Result<()> {
    let sharing_count: i64 =
        sqlx::query("SELECT COUNT(*) as c FROM sharing_participants WHERE budget_id = ?")
            .bind(sharing_id)
            .fetch_one(pool)
            .await?
            .get("c");
    let expense_count: i64 =
        sqlx::query("SELECT COUNT(*) as c FROM sharing_expenses WHERE budget_id = ?")
            .bind(sharing_id)
            .fetch_one(pool)
            .await?
            .get("c");
    let asset_count: i64 =
        sqlx::query("SELECT COUNT(*) as c FROM portfolio_assets WHERE budget_id = ?")
            .bind(invest_id)
            .fetch_one(pool)
            .await?
            .get("c");

    println!("\n=== Verification ===");
    println!("sharing budget participants: {} (need >= 2)", sharing_count);
    println!("sharing budget expenses: {} (need >= 1)", expense_count);
    println!("invest budget assets: {} (need >= 1)", asset_count);

    if sharing_count < 2 || expense_count < 1 || asset_count < 1 {
        println!("WARNING: Some seed data is missing — QA surfaces may not be fully testable.");
    } else {
        println!("All QA seed data verified OK.");
    }
    Ok(())
}
