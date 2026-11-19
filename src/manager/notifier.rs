//! Fire-and-log access to the notify outbox (see budget's notifier for the rationale).
//!
//! Sharing participants are either members (identity users, `user_id` bare or `u_`-prefixed) or
//! guests (`g_…`). Only members have an inbox, so guests are dropped from every recipient list.

use philand_event::{MySqlOutbox, NotifyCategory, NotifyEvent, NotifySeverity};

const SOURCE: &str = "sharing";
const EXCERPT_CHARS: usize = 80;

#[derive(Clone)]
pub struct Notifier {
    outbox: MySqlOutbox,
    pool: sqlx::MySqlPool,
}

impl Notifier {
    pub fn new(outbox: MySqlOutbox, pool: sqlx::MySqlPool) -> Self {
        Self { outbox, pool }
    }

    /// Enqueue `event`; does nothing when it has no recipients.
    pub async fn send(&self, event: NotifyEvent) {
        if !event.has_recipients() {
            return;
        }
        let kind = event.event_type.clone();
        if let Err(e) = self.outbox.enqueue(&self.pool, &event).await {
            tracing::warn!("notify: could not enqueue {kind}: {e}");
        }
    }
}

/// The identity user id behind a sharing participant id, or `None` for guests / empty ids.
pub fn member_user_id(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.starts_with("g_") {
        return None;
    }
    Some(raw.strip_prefix("u_").unwrap_or(raw).to_string())
}

/// Map participant ids to member user ids (guests dropped).
pub fn member_ids<'a>(raw: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    raw.into_iter().filter_map(member_user_id).collect()
}

fn excerpt(text: &str) -> String {
    let trimmed = text.trim();
    let mut out: String = trimmed.chars().take(EXCERPT_CHARS).collect();
    if trimmed.chars().count() > EXCERPT_CHARS {
        out.push('…');
    }
    out
}

/// An expense was added: whoever owes a share or paid it, minus the author. Unread ones in the
/// same budget collapse into one notification.
pub fn expense_added_event(
    budget_id: &str,
    expense_id: &str,
    actor: &str,
    actor_name: &str,
    participants: Vec<String>,
    description: &str,
    amount: i64,
) -> NotifyEvent {
    let actor = member_user_id(actor).unwrap_or_else(|| actor.to_string());
    NotifyEvent::new(
        SOURCE,
        "sharing.expense_added",
        NotifyCategory::Activity,
        participants,
        "New shared expense",
    )
    .severity(NotifySeverity::Info)
    .actor(actor)
    .budget(budget_id)
    .link(format!("/budgets/{budget_id}"))
    .group_key(format!("sharing_expense:{budget_id}"))
    .params(serde_json::json!({
        "expense_id": expense_id,
        "actor_name": actor_name,
        "description": excerpt(description),
        "amount": amount,
    }))
    .without_actor()
}

/// A settlement was marked between two participants; the other party hears about it.
pub fn settlement_marked_event(
    budget_id: &str,
    settlement_id: &str,
    actor: &str,
    actor_name: &str,
    parties: Vec<String>,
    amount: i64,
) -> NotifyEvent {
    let actor = member_user_id(actor).unwrap_or_else(|| actor.to_string());
    NotifyEvent::new(
        SOURCE,
        "sharing.settlement_marked",
        NotifyCategory::Activity,
        parties,
        "Settlement recorded",
    )
    .severity(NotifySeverity::Info)
    .actor(actor)
    .budget(budget_id)
    .link(format!("/budgets/{budget_id}"))
    .dedup_key(format!("sharing_settlement:{settlement_id}"))
    .params(serde_json::json!({
        "settlement_id": settlement_id,
        "actor_name": actor_name,
        "amount": amount,
    }))
    .without_actor()
}

/// A comment was added to an expense; its creator, payer and split members hear about it.
pub fn comment_added_event(
    budget_id: &str,
    expense_id: &str,
    actor: &str,
    actor_name: &str,
    participants: Vec<String>,
    body: &str,
) -> NotifyEvent {
    let actor = member_user_id(actor).unwrap_or_else(|| actor.to_string());
    NotifyEvent::new(
        SOURCE,
        "sharing.comment_added",
        NotifyCategory::Activity,
        participants,
        "New comment on a shared expense",
    )
    .severity(NotifySeverity::Info)
    .actor(actor)
    .budget(budget_id)
    .link(format!("/budgets/{budget_id}"))
    .group_key(format!("sharing_comment:{expense_id}"))
    .params(serde_json::json!({
        "expense_id": expense_id,
        "actor_name": actor_name,
        "excerpt": excerpt(body),
    }))
    .without_actor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guests_and_prefixes() {
        assert_eq!(member_user_id("g_abc"), None);
        assert_eq!(member_user_id(""), None);
        assert_eq!(member_user_id("u_123").as_deref(), Some("123"));
        assert_eq!(member_user_id("123").as_deref(), Some("123"));
        assert_eq!(member_ids(["g_x", "u_1", "2"]), vec!["1", "2"]);
    }

    #[test]
    fn expense_event_drops_actor_and_dupes() {
        let ev = expense_added_event(
            "b1",
            "e1",
            "1",
            "An",
            member_ids(["u_1", "2", "2", "g_z"]),
            "dinner",
            300,
        );
        assert_eq!(ev.recipient_user_ids, vec!["2"]);
        assert_eq!(ev.event_type, "sharing.expense_added");
    }

    #[test]
    fn settlement_only_other_party() {
        let ev = settlement_marked_event("b1", "s1", "1", "An", member_ids(["1", "g_q"]), 5);
        assert!(!ev.has_recipients());
        let ev = settlement_marked_event("b1", "s1", "1", "An", member_ids(["1", "2"]), 5);
        assert_eq!(ev.recipient_user_ids, vec!["2"]);
    }

    #[test]
    fn long_comment_is_cut() {
        let ev = comment_added_event("b", "e", "1", "An", vec!["2".into()], &"x".repeat(200));
        let ex = ev.params["excerpt"].as_str().unwrap().to_string();
        assert_eq!(ex.chars().count(), EXCERPT_CHARS + 1);
    }
}
