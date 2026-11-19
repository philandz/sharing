# sharing

## Notifications (producer)

Publishes personal notifications to the `notification` service through a transactional outbox
(`sharing_notify_outbox`, drained by `libs/event`). **Off by default**; set `NOTIFY_PUBLISH_ENABLED=true` together
with `NOTIFICATION_GRPC_URL` (e.g. `http://notification.philandz.svc.cluster.local:50109`) and
`INTERNAL_SERVICE_TOKEN`. Drainer tuning: `NOTIFY_OUTBOX_INTERVAL_SECS`, `_BATCH_SIZE`, `_MAX_ATTEMPTS`, `_RETENTION_SECS`.

Events: `sharing.expense_added`, `sharing.settlement_marked`, `sharing.comment_added`. Only member participants are notified; guests (`g_…`) and the actor are dropped.

Test: `DATABASE_URL=… cargo test --test notify_producer_test -- --ignored`.

Enqueue happens after the main change has committed and is best-effort (failures are logged).
Check the outbox: `SELECT event_type, attempts, last_error FROM sharing_notify_outbox WHERE delivered_at IS NULL;`
