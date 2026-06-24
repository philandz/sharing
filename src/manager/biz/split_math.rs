//! Pure split-math for sharing expenses.
//!
//! `compute_split` is a side-effect-free function that converts a split
//! request into per-user amounts. The property invariant for every
//! successful case is:
//!
//!     sum(amounts) == total
//!
//! Remainders from integer division are absorbed by the largest
//! participant (or the participant with the largest basis-points
//! share for PERCENTAGE / largest numerator for BY_ITEM) so that the
//! invariant holds exactly.

use std::collections::HashMap;

use crate::pb::service::sharing::SplitMethod;

/// Per-item input shape used by BY_ITEM splits. Mirrors the field set
/// used by `SharingBiz::add_expense`.
#[derive(Debug, Clone)]
pub struct ByItemInput {
    pub label: String,
    pub amount: i64,
    pub assignments: Vec<ItemAssignmentInput>,
}

#[derive(Debug, Clone)]
pub struct ItemAssignmentInput {
    pub user_id: String,
    pub numerator: i32,
}

/// One computed leg: `(user_id, amount, weight)`.
///
/// `weight` is the original weight / percentage / 0 — preserved for
/// audit logs but not used downstream.
pub type Leg = (String, i64, i64);

/// Compute per-user amounts for the given split method.
///
/// Returns `Err` if the inputs are invalid (e.g. CUSTOM legs don't sum
/// to the total, percentages don't sum to 10000 bp, items sum doesn't
/// match total). On success the returned vector contains one entry per
/// participant with the assigned amount.
pub fn compute_split(
    method: SplitMethod,
    total: i64,
    legs: &[Leg],
    items: &[ByItemInput],
) -> Result<Vec<Leg>, String> {
    match method {
        SplitMethod::Equal => {
            if legs.is_empty() {
                return Err("EQUAL split requires at least one leg".into());
            }
            // For EQUAL the per-leg amount is total/n, with the rounding
            // remainder absorbed by the lex-last user_id (deterministic).
            // We ignore any caller-supplied `amount` value because the
            // contract of EQUAL is "split evenly" — the caller should not
            // be pre-dividing. (For explicit per-leg amounts use CUSTOM.)
            let n = legs.len() as i64;
            let per = total / n;
            let remainder = total - per * n;
            let mut sorted: Vec<Leg> = legs.to_vec();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let len = sorted.len();
            let out: Vec<Leg> = sorted
                .into_iter()
                .enumerate()
                .map(|(i, (uid, _, w))| {
                    let extra = if (i as i64) >= len as i64 - remainder {
                        1
                    } else {
                        0
                    };
                    (uid, per + extra, w)
                })
                .collect();
            Ok(out)
        }
        SplitMethod::Custom => {
            let sum: i64 = legs.iter().map(|(_, a, _)| a).sum();
            if sum != total {
                return Err(format!("legs sum ({sum}) must equal total ({total})"));
            }
            Ok(legs.to_vec())
        }
        SplitMethod::Weighted => {
            let total_w: i64 = legs.iter().map(|(_, _, w)| w).sum();
            if total_w <= 0 {
                return Err("total weight must be > 0".into());
            }
            Ok(distribute_by(total, legs, total_w))
        }
        SplitMethod::Percentage => {
            let total_pct: i64 = legs.iter().map(|(_, _, p)| p).sum();
            if total_pct != 10_000 {
                return Err(format!("percentages must sum to 10000 (got {total_pct})"));
            }
            Ok(distribute_by(total, legs, 10_000))
        }
        SplitMethod::ByItem => {
            if items.is_empty() {
                return Err("BY_ITEM requires at least one item".into());
            }
            let items_sum: i64 = items.iter().map(|it| it.amount).sum();
            if items_sum != total {
                return Err(format!(
                    "items sum ({items_sum}) must equal total ({total})"
                ));
            }
            let mut per_user: HashMap<String, i64> = HashMap::new();
            for item in items {
                let denom: i64 = item.assignments.iter().map(|a| a.numerator as i64).sum();
                if denom <= 0 {
                    return Err(format!(
                        "item '{}' has no positive share assignments",
                        item.label
                    ));
                }
                let mut sorted = item.assignments.clone();
                // Sort by (numerator ASC, user_id ASC) so the
                // largest-share leg lands at the end of the iteration
                // and absorbs the rounding remainder (the last iteration
                // receives `item.amount - (assigned - share)`). Ties on
                // numerator resolve to the lex-earlier user_id for
                // determinism.
                sorted.sort_by(|a, b| {
                    a.numerator
                        .cmp(&b.numerator)
                        .then(a.user_id.cmp(&b.user_id))
                });
                let mut assigned: i64 = 0;
                let len = sorted.len();
                for (i, a) in sorted.iter().enumerate() {
                    let share = item.amount * (a.numerator as i64) / denom;
                    assigned += share;
                    let final_amount = if i == len - 1 {
                        item.amount - (assigned - share)
                    } else {
                        share
                    };
                    *per_user.entry(a.user_id.clone()).or_insert(0) += final_amount;
                }
            }
            Ok(per_user
                .into_iter()
                .map(|(uid, amt)| (uid, amt, 0i64))
                .collect())
        }
        SplitMethod::Unspecified => Err(
            "split_method must be specified (EQUAL / CUSTOM / WEIGHTED / PERCENTAGE / BY_ITEM)"
                .into(),
        ),
    }
}

/// Distribute `total` across `legs` in proportion to the per-leg
/// key (weight or percentage, stored in the third tuple slot).
/// Sort by user_id for determinism, then let the last entry absorb
/// the rounding remainder so that `sum(amounts) == total`.
fn distribute_by(total: i64, legs: &[Leg], key_sum: i64) -> Vec<Leg> {
    let mut sorted: Vec<Leg> = legs.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let len = sorted.len();
    let mut assigned: i64 = 0;
    let mut out: Vec<Leg> = Vec::with_capacity(len);
    for (i, (uid, _, k)) in sorted.into_iter().enumerate() {
        let share = total * k / key_sum;
        assigned += share;
        let amount = if i == len - 1 {
            total - (assigned - share)
        } else {
            share
        };
        out.push((uid, amount, k));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_sums_to(out: &[Leg], total: i64) {
        let sum: i64 = out.iter().map(|(_, a, _)| a).sum();
        assert_eq!(
            sum, total,
            "sum(legs) must equal total: got {sum}, want {total}"
        );
    }

    fn assert_non_negative(out: &[Leg]) {
        for (uid, amt, _) in out {
            assert!(*amt >= 0, "user {uid} has negative amount {amt}");
        }
    }

    // --- EQUAL ------------------------------------------------------------

    #[test]
    fn equal_divides_evenly_no_remainder() {
        let legs = vec![("a".into(), 0, 0), ("b".into(), 0, 0), ("c".into(), 0, 0)];
        let out = compute_split(SplitMethod::Equal, 99, &legs, &[]).unwrap();
        assert_sums_to(&out, 99);
        assert_eq!(out.len(), 3);
        for (_, amt, _) in &out {
            assert_eq!(*amt, 33);
        }
    }

    #[test]
    fn equal_absorbs_remainder_in_lex_last_user() {
        // 100 / 3 = 33r1 — last user (c) absorbs the +1.
        let legs = vec![("a".into(), 0, 0), ("b".into(), 0, 0), ("c".into(), 0, 0)];
        let out = compute_split(SplitMethod::Equal, 100, &legs, &[]).unwrap();
        assert_sums_to(&out, 100);
        let a = out.iter().find(|(u, _, _)| u == "a").unwrap().1;
        let b = out.iter().find(|(u, _, _)| u == "b").unwrap().1;
        let c = out.iter().find(|(u, _, _)| u == "c").unwrap().1;
        assert_eq!((a, b, c), (33, 33, 34));
    }

    #[test]
    fn equal_rejects_empty_legs() {
        let err = compute_split(SplitMethod::Equal, 100, &[], &[]).unwrap_err();
        assert!(err.contains("at least one leg"));
    }

    // --- CUSTOM -----------------------------------------------------------

    #[test]
    fn custom_rejects_wrong_sum() {
        let legs = vec![("a".into(), 60, 0), ("b".into(), 30, 0)];
        let err = compute_split(SplitMethod::Custom, 100, &legs, &[]).unwrap_err();
        assert!(err.contains("legs sum"));
    }

    // --- WEIGHTED ---------------------------------------------------------

    #[test]
    fn weighted_basic() {
        // weights 1:2 → 33/67
        let legs = vec![("a".into(), 0, 1), ("b".into(), 0, 2)];
        let out = compute_split(SplitMethod::Weighted, 100, &legs, &[]).unwrap();
        assert_sums_to(&out, 100);
        let a = out.iter().find(|(u, _, _)| u == "a").unwrap().1;
        let b = out.iter().find(|(u, _, _)| u == "b").unwrap().1;
        assert_eq!(a + b, 100);
    }

    #[test]
    fn weighted_rejects_zero_weight() {
        let legs = vec![("a".into(), 0, 0), ("b".into(), 0, 0)];
        assert!(compute_split(SplitMethod::Weighted, 100, &legs, &[]).is_err());
    }

    #[test]
    fn weighted_uneven_three_users() {
        let legs = vec![("a".into(), 0, 3), ("b".into(), 0, 1), ("c".into(), 0, 1)];
        let out = compute_split(SplitMethod::Weighted, 100, &legs, &[]).unwrap();
        assert_sums_to(&out, 100);
        assert_non_negative(&out);
    }

    // --- PERCENTAGE -------------------------------------------------------

    #[test]
    fn percentage_basic() {
        // 2500 + 7500 = 10000 bp
        let legs = vec![("a".into(), 0, 2500), ("b".into(), 0, 7500)];
        let out = compute_split(SplitMethod::Percentage, 200, &legs, &[]).unwrap();
        assert_sums_to(&out, 200);
        let a = out.iter().find(|(u, _, _)| u == "a").unwrap().1;
        let b = out.iter().find(|(u, _, _)| u == "b").unwrap().1;
        assert_eq!(a, 50);
        assert_eq!(b, 150);
    }

    #[test]
    fn percentage_rejects_wrong_sum() {
        let legs = vec![("a".into(), 0, 5000), ("b".into(), 0, 4000)];
        assert!(compute_split(SplitMethod::Percentage, 100, &legs, &[]).is_err());
    }

    // --- BY_ITEM ----------------------------------------------------------

    #[test]
    fn by_item_basic() {
        // Dinner 60 split between a,b; Taxi 40 split between a,b,c
        let items = vec![
            ByItemInput {
                label: "Dinner".into(),
                amount: 60,
                assignments: vec![
                    ItemAssignmentInput {
                        user_id: "a".into(),
                        numerator: 1,
                    },
                    ItemAssignmentInput {
                        user_id: "b".into(),
                        numerator: 1,
                    },
                ],
            },
            ByItemInput {
                label: "Taxi".into(),
                amount: 40,
                assignments: vec![
                    ItemAssignmentInput {
                        user_id: "a".into(),
                        numerator: 1,
                    },
                    ItemAssignmentInput {
                        user_id: "b".into(),
                        numerator: 1,
                    },
                    ItemAssignmentInput {
                        user_id: "c".into(),
                        numerator: 1,
                    },
                ],
            },
        ];
        let out = compute_split(SplitMethod::ByItem, 100, &[], &items).unwrap();
        assert_sums_to(&out, 100);
        // a: 30 + 13 = 43, b: 30 + 13 = 43, c: 0 + 14 = 14 → 43+43+14 = 100
        let a = out.iter().find(|(u, _, _)| u == "a").unwrap().1;
        let b = out.iter().find(|(u, _, _)| u == "b").unwrap().1;
        let c = out.iter().find(|(u, _, _)| u == "c").unwrap().1;
        assert_eq!(a, 43);
        assert_eq!(b, 43);
        assert_eq!(c, 14);
    }

    #[test]
    fn by_item_largest_share_absorbs_remainder() {
        // Two users, item amount 100. z has weight 1, a has weight 2.
        // Naive share: a=66, z=33 → sums to 99. The remainder (1) must
        // be absorbed by the largest-share leg (a), not the lex-last
        // user_id (z). After fix: a=67, z=33.
        let items = vec![ByItemInput {
            label: "x".into(),
            amount: 100,
            assignments: vec![
                ItemAssignmentInput {
                    user_id: "z".into(),
                    numerator: 1,
                },
                ItemAssignmentInput {
                    user_id: "a".into(),
                    numerator: 2,
                },
            ],
        }];
        let out = compute_split(SplitMethod::ByItem, 100, &[], &items).unwrap();
        assert_sums_to(&out, 100);
        let a = out.iter().find(|(u, _, _)| u == "a").unwrap().1;
        let z = out.iter().find(|(u, _, _)| u == "z").unwrap().1;
        assert_eq!(
            a, 67,
            "largest-share leg (a) should absorb the +1 remainder"
        );
        assert_eq!(z, 33);
    }

    #[test]
    fn by_item_rejects_zero_numerator() {
        let items = vec![ByItemInput {
            label: "x".into(),
            amount: 100,
            assignments: vec![ItemAssignmentInput {
                user_id: "a".into(),
                numerator: 0,
            }],
        }];
        assert!(compute_split(SplitMethod::ByItem, 100, &[], &items).is_err());
    }

    #[test]
    fn by_item_rejects_mismatch_total() {
        let items = vec![ByItemInput {
            label: "x".into(),
            amount: 50,
            assignments: vec![ItemAssignmentInput {
                user_id: "a".into(),
                numerator: 1,
            }],
        }];
        assert!(compute_split(SplitMethod::ByItem, 100, &[], &items).is_err());
    }

    #[test]
    fn by_item_requires_at_least_one_item() {
        assert!(compute_split(SplitMethod::ByItem, 100, &[], &[]).is_err());
    }

    // --- Unspecified ------------------------------------------------------

    #[test]
    fn unspecified_rejected() {
        let legs = vec![("a".into(), 100, 0)];
        assert!(compute_split(SplitMethod::Unspecified, 100, &legs, &[]).is_err());
    }

    // --- Property tests (proptest) --------------------------------------
    //
    // For every successful split the invariant is:
    //
    //     sum(amounts) == total      (exactly, with rounding absorbed
    //                                  by the largest-share leg)
    //
    // These tests run randomized inputs across all 5 split methods and
    // assert that invariant, plus that invalid inputs are rejected.

    use proptest::prelude::*;

    /// Build n legs with the given per-leg `amount` for EQUAL/CUSTOM,
    /// or with the given per-leg `share` for WEIGHTED/PERCENTAGE.
    fn make_legs(n: usize, amount: i64, share: i64) -> Vec<Leg> {
        (0..n).map(|i| (format!("u{i}"), amount, share)).collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn prop_equal_sums_to_total(
            total in 1i64..100_000,
            n in 1usize..=6,
        ) {
            // For EQUAL we pre-distribute: each leg gets total/n.
            let per = total / (n as i64);
            let legs = make_legs(n, per, 0);
            let out = compute_split(SplitMethod::Equal, total, &legs, &[]).unwrap();
            assert_sums_to(&out, total);
            assert_non_negative(&out);
        }

        #[test]
        fn prop_custom_sums_to_total(
            total in 1i64..100_000,
            amounts in proptest::collection::vec(1i64..10_000, 1..=6),
        ) {
            // Sum amounts, then bump the first leg so the total matches.
            let mut legs: Vec<Leg> = amounts
                .iter()
                .enumerate()
                .map(|(i, a)| (format!("u{i}"), *a, 0))
                .collect();
            let sum: i64 = legs.iter().map(|(_, a, _)| a).sum();
            if sum > total {
                // Skip inputs where the random legs already exceed the total.
                return Ok(());
            }
            legs[0].1 += total - sum;
            let out = compute_split(SplitMethod::Custom, total, &legs, &[]).unwrap();
            assert_sums_to(&out, total);
            assert_non_negative(&out);
        }

        #[test]
        fn prop_custom_rejects_wrong_sum(
            total in 1i64..100_000,
            amounts in proptest::collection::vec(1i64..10_000, 2..=6),
        ) {
            // Skip if the random amounts happen to sum to total exactly.
            let sum: i64 = amounts.iter().sum();
            if sum == total {
                return Ok(());
            }
            let legs: Vec<Leg> = amounts
                .iter()
                .enumerate()
                .map(|(i, a)| (format!("u{i}"), *a, 0))
                .collect();
            let err = compute_split(SplitMethod::Custom, total, &legs, &[]).unwrap_err();
            prop_assert!(err.contains("legs sum"), "unexpected error: {err}");
        }

        #[test]
        fn prop_weighted_sums_to_total(
            total in 1i64..100_000,
            weights in proptest::collection::vec(1i64..1000, 1..=6),
        ) {
            let n = weights.len();
            let legs: Vec<Leg> = weights
                .iter()
                .enumerate()
                .map(|(i, w)| (format!("u{i}"), 0, *w))
                .collect();
            let out = compute_split(SplitMethod::Weighted, total, &legs, &[]).unwrap();
            assert_sums_to(&out, total);
            assert_non_negative(&out);
            // Sanity: at least one leg got a nonzero share.
            prop_assert!(out.iter().any(|(_, a, _)| *a > 0));
            let _ = n;
        }

        #[test]
        fn prop_percentage_sums_to_total(
            total in 1i64..100_000,
            n in 2usize..=6,
        ) {
            // Build n bp values that sum to exactly 10000 by
            // distributing 10000/n and putting the remainder on leg 0.
            let per = 10_000 / (n as i64);
            let mut bps = vec![per; n];
            bps[0] += 10_000 - per * (n as i64);
            let legs: Vec<Leg> = bps
                .iter()
                .enumerate()
                .map(|(i, p)| (format!("u{i}"), 0, *p))
                .collect();
            let out = compute_split(SplitMethod::Percentage, total, &legs, &[]).unwrap();
            assert_sums_to(&out, total);
            assert_non_negative(&out);
        }

        #[test]
        fn prop_percentage_rejects_wrong_sum(
            total in 1i64..100_000,
            bps in proptest::collection::vec(1i64..=9_999, 2..=6),
        ) {
            // Skip if bps happen to sum to 10000.
            let sum: i64 = bps.iter().sum();
            if sum == 10_000 {
                return Ok(());
            }
            let legs: Vec<Leg> = bps
                .iter()
                .enumerate()
                .map(|(i, p)| (format!("u{i}"), 0, *p))
                .collect();
            let err = compute_split(SplitMethod::Percentage, total, &legs, &[]).unwrap_err();
            prop_assert!(err.contains("10000"), "unexpected error: {err}");
        }

        #[test]
        fn prop_by_item_sums_to_total(
            total in 1i64..100_000,
            n_items in 1usize..=4,
            assignees in proptest::collection::vec(1i32..=10, 2..=4),
        ) {
            // Per-item amounts are random and split uniformly across
            // assignees, then their sum is reconciled with `total`.
            let mut items: Vec<ByItemInput> = Vec::new();
            let mut running: i64 = 0;
            for i in 0..n_items {
                let amount: i64 = ((i as i64 + 1) * 1_000) % 9_000 + 1;
                running += amount;
                items.push(ByItemInput {
                    label: format!("item{i}"),
                    amount,
                    assignments: assignees
                        .iter()
                        .enumerate()
                        .map(|(j, num)| ItemAssignmentInput {
                            user_id: format!("u{j}"),
                            numerator: *num,
                        })
                        .collect(),
                });
            }
            // Reconcile: scale the last item so items sum to `total`.
            if running == 0 {
                return Ok(());
            }
            let last = items.len() - 1;
            let desired_last = total - (running - items[last].amount);
            if desired_last <= 0 {
                return Ok(());
            }
            items[last].amount = desired_last;
            let out = compute_split(SplitMethod::ByItem, total, &[], &items).unwrap();
            assert_sums_to(&out, total);
            assert_non_negative(&out);
        }

        #[test]
        fn prop_unspecified_rejected(total in 1i64..100_000) {
            let legs = vec![("u0".into(), total, 0)];
            prop_assert!(compute_split(SplitMethod::Unspecified, total, &legs, &[]).is_err());
        }
    }
}
