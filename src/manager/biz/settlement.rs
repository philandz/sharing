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

        let (debtor_id, debtor_name, debtor_balance) =
            (signed[debtor_idx].0.clone(), signed[debtor_idx].1.clone(), signed[debtor_idx].2);
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
