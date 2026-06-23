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
        SplitMethod::Equal => Ok(legs.to_vec()),
        SplitMethod::Custom => {
            let sum: i64 = legs.iter().map(|(_, a, _)| a).sum();
            if sum != total {
                return Err(format!(
                    "legs sum ({sum}) must equal total ({total})"
                ));
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
                return Err(format!(
                    "percentages must sum to 10000 (got {total_pct})"
                ));
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
                let denom: i64 = item
                    .assignments
                    .iter()
                    .map(|a| a.numerator as i64)
                    .sum();
                if denom <= 0 {
                    return Err(format!(
                        "item '{}' has no positive share assignments",
                        item.label
                    ));
                }
                let mut sorted = item.assignments.clone();
                sorted.sort_by(|a, b| {
                    a.user_id
                        .cmp(&b.user_id)
                        .then(a.numerator.cmp(&b.numerator).reverse())
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
        assert_eq!(sum, total, "sum(legs) must equal total: got {sum}, want {total}");
    }

    fn assert_non_negative(out: &[Leg]) {
        for (uid, amt, _) in out {
            assert!(*amt >= 0, "user {uid} has negative amount {amt}");
        }
    }

    // --- EQUAL ------------------------------------------------------------

    #[test]
    fn equal_passes_through_legs() {
        let legs = vec![
            ("a".into(), 30, 0),
            ("b".into(), 30, 0),
            ("c".into(), 40, 0),
        ];
        let out = compute_split(SplitMethod::Equal, 100, &legs, &[]).unwrap();
        assert_sums_to(&out, 100);
        assert_eq!(out.len(), 3);
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
        let legs = vec![
            ("a".into(), 0, 3),
            ("b".into(), 0, 1),
            ("c".into(), 0, 1),
        ];
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
                    ItemAssignmentInput { user_id: "a".into(), numerator: 1 },
                    ItemAssignmentInput { user_id: "b".into(), numerator: 1 },
                ],
            },
            ByItemInput {
                label: "Taxi".into(),
                amount: 40,
                assignments: vec![
                    ItemAssignmentInput { user_id: "a".into(), numerator: 1 },
                    ItemAssignmentInput { user_id: "b".into(), numerator: 1 },
                    ItemAssignmentInput { user_id: "c".into(), numerator: 1 },
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
    fn by_item_rejects_zero_numerator() {
        let items = vec![ByItemInput {
            label: "x".into(),
            amount: 100,
            assignments: vec![ItemAssignmentInput { user_id: "a".into(), numerator: 0 }],
        }];
        assert!(compute_split(SplitMethod::ByItem, 100, &[], &items).is_err());
    }

    #[test]
    fn by_item_rejects_mismatch_total() {
        let items = vec![ByItemInput {
            label: "x".into(),
            amount: 50,
            assignments: vec![ItemAssignmentInput { user_id: "a".into(), numerator: 1 }],
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

    // --- Property: weighted sums to total for many random inputs ----------

    #[test]
    fn property_weighted_sums_to_total() {
        // Deterministic pseudo-random to keep the test stable.
        let mut seed: u64 = 0xDEAD_BEEF_CAFE_BABE;
        for _ in 0..50 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let n = ((seed >> 33) as usize % 4) + 1; // 1..=4 users
            let mut legs: Vec<Leg> = Vec::with_capacity(n);
            for i in 0..n {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let w = ((seed >> 20) as i64 % 100).abs() + 1;
                legs.push((format!("u{i}"), 0, w));
            }
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let total = ((seed >> 10) as i64 % 10_000) + 1;
            let out = compute_split(SplitMethod::Weighted, total, &legs, &[]).unwrap();
            assert_sums_to(&out, total);
            assert_non_negative(&out);
        }
    }

    #[test]
    fn property_percentage_sums_to_total() {
        let mut seed: u64 = 0xCAFE_F00D_BAAD_F00D;
        for _ in 0..50 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let n = ((seed >> 33) as usize % 4) + 1; // 1..=4 users
            // Build bp values summing to exactly 10000.
            let mut bps = vec![1000i64; n];
            bps[0] += 10000 - 1000 * (n as i64);
            let mut legs: Vec<Leg> = Vec::with_capacity(n);
            for i in 0..n {
                legs.push((format!("u{i}"), 0, bps[i]));
            }
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let total = ((seed >> 10) as i64 % 10_000) + 1;
            let out = compute_split(SplitMethod::Percentage, total, &legs, &[]).unwrap();
            assert_sums_to(&out, total);
            assert_non_negative(&out);
        }
    }
}
