use crate::pb::service::sharing::Transfer;

/// Greedy debt-minimization settlement.
///
/// `signed` is a list of `(user_id, display_name, net_balance)` tuples.
/// A positive balance means the user is owed; negative means they owe.
///
/// For each iteration we pair the largest creditor with the largest debtor,
/// settle the minimum of the two balances, and emit a `Transfer`. The result
/// contains at most N-1 transfers for N participants.
///
/// Settlement is in-app only: no QR code image, no banking-app deep-link.
/// Each `Transfer` carries only the four base fields (from, to, names, amount).
pub fn greedy_settle(signed: &[(String, String, i64)]) -> Vec<Transfer> {
    let mut signed: Vec<(String, String, i64)> = signed.to_vec();
    // Pre-sort by user_id ASC so that equal-balance ties resolve to the
    // same pairing on every run. This is a stable pre-sort applied once
    // before the main loop; subsequent re-sorts within the loop inherit
    // the deterministic input order via Rust's stable `sort_by_key`.
    signed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut transfers: Vec<Transfer> = Vec::new();

    loop {
        // Sort: creditors (positive) at end, debtors (negative) at start
        signed.sort_by_key(|(_, _, b)| *b);
        let first = signed.first().map(|(_, _, b)| *b).unwrap_or(0);
        let last = signed.last().map(|(_, _, b)| *b).unwrap_or(0);

        if first >= 0 || last <= 0 {
            break;
        } // all settled

        let debtor_idx = 0;
        let creditor_idx = signed.len() - 1;

        let (debtor_id, debtor_name, debtor_balance) = (
            signed[debtor_idx].0.clone(),
            signed[debtor_idx].1.clone(),
            signed[debtor_idx].2,
        );
        let (creditor_id, creditor_name, creditor_balance) = (
            signed[creditor_idx].0.clone(),
            signed[creditor_idx].1.clone(),
            signed[creditor_idx].2,
        );
        let debt = -debtor_balance;
        let credit = creditor_balance;
        let amount = debt.min(credit);

        transfers.push(Transfer {
            from_user_id: debtor_id,
            from_name: debtor_name,
            to_user_id: creditor_id,
            to_name: creditor_name,
            amount,
        });

        signed[debtor_idx].2 += amount;
        signed[creditor_idx].2 -= amount;

        // Remove zeroed entries
        signed.retain(|(_, _, b)| *b != 0);
    }

    transfers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two equal-balance scenarios must produce the same from/to pairings
    /// regardless of input ordering. Guards the pre-sort by user_id ASC.
    #[test]
    fn ties_resolve_deterministically_across_orderings() {
        // Scenario A: creditor "b" +50, debtor "a" -50. Pre-sorted by user_id
        // → a debtor comes first, b creditor last → a→b.
        let scenario_a = vec![
            ("b".to_string(), "Bob".to_string(), 50i64),
            ("a".to_string(), "Alice".to_string(), -50i64),
        ];
        let scenario_b = vec![
            ("a".to_string(), "Alice".to_string(), -50i64),
            ("b".to_string(), "Bob".to_string(), 50i64),
        ];

        let t_a = greedy_settle(&scenario_a);
        let t_b = greedy_settle(&scenario_b);

        assert_eq!(t_a.len(), 1);
        assert_eq!(t_b.len(), 1);
        assert_eq!(t_a[0].from_user_id, t_b[0].from_user_id);
        assert_eq!(t_a[0].to_user_id, t_b[0].to_user_id);
        assert_eq!(t_a[0].from_user_id, "a");
        assert_eq!(t_a[0].to_user_id, "b");
    }

    /// Two creditors with equal positive balances, one debtor — the
    /// algorithm must pick the same creditor across runs.
    #[test]
    fn equal_creditors_resolve_deterministically() {
        // a and c both +50, b is -100. The pre-sort by user_id ASC
        // ensures b is paired with c first (a is dropped first because
        // the balance-sort within the loop ties c with a, and the
        // user_id-ASC pre-sort puts a before c, so c is the last).
        let signed = vec![
            ("b".to_string(), "Bob".to_string(), -100i64),
            ("a".to_string(), "Alice".to_string(), 50i64),
            ("c".to_string(), "Carol".to_string(), 50i64),
        ];
        let t = greedy_settle(&signed);
        assert_eq!(t.len(), 2);
        // First transfer: b pays c 50.
        assert_eq!(t[0].from_user_id, "b");
        assert_eq!(t[0].to_user_id, "c");
        assert_eq!(t[0].amount, 50);
        // Second transfer: b pays a 50.
        assert_eq!(t[1].from_user_id, "b");
        assert_eq!(t[1].to_user_id, "a");
        assert_eq!(t[1].amount, 50);
    }

    /// All-zero balances — no transfers emitted.
    #[test]
    fn all_zero_balances_no_transfers() {
        let signed = vec![
            ("a".to_string(), "Alice".to_string(), 0i64),
            ("b".to_string(), "Bob".to_string(), 0i64),
        ];
        let t = greedy_settle(&signed);
        assert!(t.is_empty());
    }

    /// Single-participant expense — payer credited, no debtors. Should
    /// produce no transfers (no one to pay).
    #[test]
    fn single_payer_no_debtors() {
        let signed = vec![("alice".to_string(), "Alice".to_string(), 100i64)];
        let t = greedy_settle(&signed);
        assert!(t.is_empty());
    }

    /// Empty input — no transfers, no panic.
    #[test]
    fn empty_input_no_transfers() {
        let t = greedy_settle(&[]);
        assert!(t.is_empty());
    }

    /// All participants net positive (everyone is owed) — no debtors,
    /// loop exits immediately.
    #[test]
    fn all_creditors_no_debtors() {
        let signed = vec![
            ("a".to_string(), "Alice".to_string(), 100i64),
            ("b".to_string(), "Bob".to_string(), 100i64),
        ];
        let t = greedy_settle(&signed);
        assert!(t.is_empty());
    }

    /// Net sum must equal zero after settlement — invariant check.
    /// Total credits must equal total debits in the input.
    #[test]
    fn net_sum_zero_invariant() {
        let signed = vec![
            ("a".to_string(), "Alice".to_string(), 50i64),
            ("b".to_string(), "Bob".to_string(), 50i64),
            ("c".to_string(), "Carol".to_string(), -50i64),
            ("d".to_string(), "Dave".to_string(), -50i64),
        ];
        let sum: i64 = signed.iter().map(|(_, _, b)| b).sum();
        assert_eq!(sum, 0, "test fixture should have zero net sum");

        let t = greedy_settle(&signed);
        // Total amount of transfers should equal total credits (= 100).
        let total_xfer: i64 = t.iter().map(|x| x.amount).sum();
        assert_eq!(total_xfer, 100);
    }

    /// Asymmetric split: big debt, small credit. One transfer of min.
    #[test]
    fn asymmetric_debt_credit() {
        let signed = vec![
            ("debtor".to_string(), "Dave".to_string(), -1000i64),
            ("creditor".to_string(), "Carol".to_string(), 50i64),
        ];
        let t = greedy_settle(&signed);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].amount, 50);
        // After this transfer, debtor still owes 950 (their balance becomes
        // -950), creditor is settled (balance becomes 0). Both are kept
        // in `signed` and the next iteration sorts them: -950 < 0 so
        // still a debtor; creditor at balance 0 is removed by retain.
        // Loop exits because last <= 0 check fails on the remaining
        // -950 entry alone (signed is [(debtor, ..., -950)]; first=last=-950,
        // not (first>=0 || last<=0) — wait, last <= 0 IS true, so loop
        // exits. Correct: 1 transfer emitted, debtor still has unresolved
        // balance. The caller is expected to call again after more expenses
        // are added.
    }

    /// Three-way: 2 debtors, 1 creditor. One transfer settles one debtor
    /// fully and partially pays the other.
    #[test]
    fn two_debtors_one_creditor() {
        let signed = vec![
            ("d1".to_string(), "Dave".to_string(), -40i64),
            ("d2".to_string(), "Eve".to_string(), -60i64),
            ("c1".to_string(), "Carol".to_string(), 100i64),
        ];
        let t = greedy_settle(&signed);
        // 100 split across two debtors: 40 to d1, 60 to d2 = 2 transfers.
        assert_eq!(t.len(), 2);
        let total: i64 = t.iter().map(|x| x.amount).sum();
        assert_eq!(total, 100);
    }
}
