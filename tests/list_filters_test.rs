//! Filtered/paged ListExpenses against a real MySQL.
//! `DATABASE_URL=... cargo test --test list_filters_test -- --ignored`

use sharing::manager::repository::{ExpenseFilter, ExpenseScope, ExpenseSort, SharingRepository};
use sharing::pb::service::sharing::SplitMethod;

trait HasDesc {
    fn desc(&self) -> String;
}
impl HasDesc for sharing::converters::DbExpense {
    fn desc(&self) -> String {
        self.description.clone()
    }
}

fn legs(parts: &[(&str, i64)]) -> Vec<(String, i64, i64)> {
    parts.iter().map(|(u, a)| (u.to_string(), *a, 0)).collect()
}

#[tokio::test]
#[ignore = "needs DATABASE_URL"]
async fn filters_sort_and_paging_work_against_real_sql() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let repo = SharingRepository::new(&url).await.unwrap();
    let b = uuid::Uuid::new_v4().to_string();
    let other = uuid::Uuid::new_v4().to_string();
    let (a, bo, c) = ("ua", "ub", "uc");

    // date, payer, amount, description, legs
    let rows: [(&str, &str, i64, &str, Vec<(String, i64, i64)>); 5] = [
        (
            "2026-10-01",
            a,
            300,
            "Dinner 50% off",
            legs(&[(a, 100), (bo, 100), (c, 100)]),
        ),
        ("2026-10-15", bo, 200, "Taxi", legs(&[(bo, 100), (c, 100)])),
        ("2026-11-02", a, 1500, "Hotel", legs(&[(a, 750), (bo, 750)])),
        ("2026-11-02", c, 90, "Snack_bar", legs(&[(c, 90)])), // personal: only the payer
        ("2026-11-20", a, 1000, "Gift", legs(&[(a, 1000)])),  // personal
    ];
    for (d, payer, amt, desc, l) in &rows {
        repo.create_expense(&b, payer, *amt, desc, d, None, SplitMethod::Equal, l, payer)
            .await
            .unwrap();
    }
    // noise in another budget must never leak in
    repo.create_expense(
        &other,
        a,
        999,
        "Other",
        "2026-11-02",
        None,
        SplitMethod::Equal,
        &legs(&[(a, 999)]),
        a,
    )
    .await
    .unwrap();

    let list = |f: ExpenseFilter| {
        let repo = &repo;
        let b = b.clone();
        async move { repo.list_expenses_filtered(&b, &f).await.unwrap() }
    };
    let names = |r: &(Vec<sharing::converters::DbExpense>, i64, i64)| {
        r.0.iter()
            .map(|e| e.description.clone())
            .collect::<Vec<_>>()
    };

    // 1. no filter: everything in this budget only, newest first, tie broken stably
    let all = list(ExpenseFilter::default()).await;
    assert_eq!((all.1, all.2), (5, 3090));
    assert_eq!(names(&all)[0], "Gift");
    assert_eq!(all.0.len(), 5);

    // 2. inclusive date range
    let nov = list(ExpenseFilter {
        date_from: Some("2026-11-01".into()),
        date_to: Some("2026-11-02".into()),
        ..Default::default()
    })
    .await;
    assert_eq!(nov.1, 2);

    // 3. payer (OR) and the leg EXISTS
    let paid = list(ExpenseFilter {
        paid_by_ids: vec![bo.into(), c.into()],
        ..Default::default()
    })
    .await;
    assert_eq!(paid.1, 2);
    let has_c = list(ExpenseFilter {
        share_participant_ids: vec![c.into()],
        ..Default::default()
    })
    .await;
    assert_eq!(has_c.1, 3, "Dinner, Taxi, Snack_bar");

    // 4. scope
    let shared = list(ExpenseFilter {
        scope: ExpenseScope::Shared,
        ..Default::default()
    })
    .await;
    assert_eq!(shared.1, 3);
    let personal = list(ExpenseFilter {
        scope: ExpenseScope::Personal,
        ..Default::default()
    })
    .await;
    assert_eq!(personal.1, 2);

    // 5. text: literal % and _, and exact amount
    let pct = list(ExpenseFilter {
        q: Some("50%".into()),
        ..Default::default()
    })
    .await;
    assert_eq!(names(&pct), vec!["Dinner 50% off"]);
    let under = list(ExpenseFilter {
        q: Some("T_xi".into()),
        ..Default::default()
    })
    .await;
    assert_eq!(under.1, 0, "_ must not act as a wildcard");
    let amt = list(ExpenseFilter {
        q: Some("1.500".into()),
        ..Default::default()
    })
    .await;
    assert_eq!(names(&amt), vec!["Hotel"]);

    // 6. sort by amount and paging with a total that ignores the page
    let page2 = list(ExpenseFilter {
        sort_by: ExpenseSort::Amount,
        page: 2,
        page_size: 2,
        ..Default::default()
    })
    .await;
    assert_eq!((page2.1, page2.2), (5, 3090));
    assert_eq!(names(&page2), vec!["Dinner 50% off", "Taxi"]);
    let asc = list(ExpenseFilter {
        sort_by: ExpenseSort::Amount,
        ascending: true,
        page_size: 1,
        page: 1,
        ..Default::default()
    })
    .await;
    assert_eq!(names(&asc), vec!["Snack_bar"]);

    // 7. same-day rows page without repeats or gaps
    let mut seen = vec![];
    for p in 1..=3 {
        seen.extend(names(
            &list(ExpenseFilter {
                page: p,
                page_size: 2,
                ..Default::default()
            })
            .await,
        ));
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5);
}
