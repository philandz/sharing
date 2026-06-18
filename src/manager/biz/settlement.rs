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
/// `qr_image_base` is the VietQR image-generation base (e.g.
/// `https://img.vietqr.io/image`) used to build a QR code deep-link for display.
///
/// `payment_base` is the VietQR banking-app scheme (e.g. `vietqr://pay`) used
/// to build a deep-link that the user's banking app can open directly.
pub fn greedy_settle(
    signed: &[(String, String, i64)],
    qr_image_base: &str,
    payment_base: &str,
) -> Vec<Transfer> {
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

        // QR image deep-link (for display). Use chars().take(8) so short
        // user_ids (e.g. "u1") don't panic on slice indexing.
        let deep_link = format!(
            "{}/napas247-{}-TRANSFER.jpg?amount={}&addInfo=Settle+sharing+budget",
            qr_image_base,
            creditor_id.chars().take(8).collect::<String>(),
            amount,
        );

        // Banking-app deep-link (for action). Uses a different base and a
        // separate format string from the QR image URL.
        let payment_url = format!(
            "{}?from={}&to={}&amount={}&addInfo=Settle+sharing+budget",
            payment_base,
            debtor_id,
            creditor_id,
            amount,
        );

        transfers.push(Transfer {
            from_user_id: debtor_id,
            from_name: debtor_name,
            to_user_id: creditor_id,
            to_name: creditor_name,
            amount,
            deep_link,
            payment_url,
        });

        signed[debtor_idx].2 += amount;
        signed[creditor_idx].2 -= amount;

        // Remove zeroed entries
        signed.retain(|(_, _, b)| *b != 0);
    }

    transfers
}