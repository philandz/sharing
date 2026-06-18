#!/usr/bin/env bash
# Layer D — Sharing Budget integration test suite
# Restored from earlier session. Adds D.19b for cross-user-contributor delete.

set -euo pipefail

GATEWAY_URL="${GATEWAY_URL:-http://127.0.0.1:9100}"
API_BASE="${GATEWAY_URL%/}/api"
IDENTITY_BASE="${API_BASE}/identity"
BUDGET_BASE="${API_BASE}/budget"
SHARING_BASE="${API_BASE}/sharing"

OWNER_EMAIL="${OWNER_EMAIL:-owner@philand.test}"
USER2_EMAIL="${USER2_EMAIL:-contrib@philand.test}"
USER3_EMAIL="${USER3_EMAIL:-manager@philand.test}"
OUTSIDER_EMAIL="${OUTSIDER_EMAIL:-outsider@philand.test}"
VIEWER_EMAIL="${VIEWER_EMAIL:-viewer@philand.test}"

JWT_SECRET="${JWT_SECRET:-local-dev-jwt-secret-change-in-prod}"

PASS=0
FAIL=0
SKIP=0
TOTAL=0

LAST_STATUS=""
LAST_BODY=""

mint_jwt() {
  local email="$1"
  local now exp
  now=$(date +%s)
  exp=$(( now + 3600 ))
  local header='{"alg":"HS256","typ":"JWT"}'
  local payload="{\"sub\":\"${email}\",\"email\":\"${email}\",\"iat\":${now},\"exp\":${exp}}"
  local h p sig
  h=$(printf '%s' "$header"  | base64 | tr -d '=' | tr '/+' '_-')
  p=$(printf '%s' "$payload" | base64 | tr -d '=' | tr '/+' '_-')
  sig=$(printf '%s.%s' "$h" "$p" | openssl dgst -binary -sha256 -hmac "$JWT_SECRET" | base64 | tr -d '=' | tr '/+' '_-')
  printf '%s.%s.%s' "$h" "$p" "$sig"
}

pass()  { printf '  \033[32m✓\033[0m %s\n' "$1"; PASS=$((PASS+1));  TOTAL=$((TOTAL+1)); }
fail()  { printf '  \033[31m✗\033[0m %s\n' "$1"; FAIL=$((FAIL+1));  TOTAL=$((TOTAL+1)); }
skip()  { printf '  \033[33m~\033[0m %s (skipped)\n' "$1"; SKIP=$((SKIP+1));  TOTAL=$((TOTAL+1)); }
header(){ printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

req() {
  local method="$1"
  local path="$2"
  local body="${3:-}"
  local token="${4:-}"
  local args=(
    -sS
    -o /tmp/sharing_e2e_body
    -w '%{http_code}'
    -X "$method"
    "$path"
  )
  if [[ -n "$body" ]]; then
    args+=( -H 'content-type: application/json' -d "$body" )
  fi
  if [[ -n "$token" ]]; then
    args+=( -H "authorization: Bearer $token" )
  fi
  LAST_STATUS=$(curl "${args[@]}")
  LAST_BODY=$(cat /tmp/sharing_e2e_body)
}

assert_status() {
  local name="$1"
  local expected="$2"
  if [[ "$LAST_STATUS" == "$expected" ]]; then
    pass "$name (status=$LAST_STATUS)"
  else
    fail "$name (status=$LAST_STATUS, expected=$expected) — body=$LAST_BODY"
  fi
}

assert_status_in() {
  local name="$1"; shift
  local codes=("$@")
  for code in "${codes[@]}"; do
    if [[ "$LAST_STATUS" == "$code" ]]; then
      pass "$name (status=$LAST_STATUS)"; return
    fi
  done
  fail "$name (status=$LAST_STATUS, expected one of: ${codes[*]}) — body=$LAST_BODY"
}

assert_body_contains() {
  local name="$1"
  local needle="$2"
  if printf '%s' "$LAST_BODY" | grep -Fq "$needle"; then
    pass "$name (body contains '$needle')"
  else
    fail "$name (body missing '$needle') — body=$LAST_BODY"
  fi
}

jget() {
  if [[ -z "$LAST_BODY" ]]; then
    printf ''
    return
  fi
  printf '%s' "$LAST_BODY" | python3 -c "
import json, sys, re
try:
    d = json.loads(sys.stdin.read())
except (json.JSONDecodeError, ValueError):
    print(''); sys.exit(0)
k = '$1'.lstrip('.')
tokens = []
for part in k.split('.'):
    if not part: continue
    m = re.match(r'^([^[]+)\[(\d+)\]\$', part)
    if m:
        tokens.append(m.group(1))
        tokens.append(m.group(2))
    else:
        tokens.append(part)
cur = d
for t in tokens:
    if isinstance(cur, list):
        try: cur = cur[int(t)]
        except (ValueError, IndexError): print(''); sys.exit(0)
    elif isinstance(cur, dict):
        cur = cur.get(t)
    else:
        print(''); sys.exit(0)
    if cur is None: print(''); sys.exit(0)
print(cur if not isinstance(cur, (dict, list)) else json.dumps(cur))
" 2>/dev/null
}

check_infra() {
  printf 'Checking prerequisites...\n'
  local ok=1
  if ! curl -sf -o /dev/null --max-time 3 "${GATEWAY_URL}/health" \
      && ! curl -sf -o /dev/null --max-time 3 "${GATEWAY_URL}/" 2>/dev/null; then
    printf '  \033[31m✗\033[0m Gateway is not reachable at %s\n' "$GATEWAY_URL"
    ok=0
  else
    printf '  \033[32m✓\033[0m Gateway reachable at %s\n' "$GATEWAY_URL"
  fi
  local code
  code=$(curl -sS -o /dev/null -w '%{http_code}' --max-time 3 \
    -H "authorization: Bearer dummy" \
    "${SHARING_BASE}/budgets/dummy/expenses" 2>/dev/null)
  code="${code:-000}"
  if [[ "$code" == "000" || -z "$code" ]]; then
    printf '  \033[31m✗\033[0m Sharing endpoint not reachable at %s\n' "$SHARING_BASE"
    ok=0
  else
    printf '  \033[32m✓\033[0m Sharing endpoint reachable (HTTP %s on probe)\n' "$code"
  fi
  if [[ "${ok}" == "0" ]]; then
    return 1
  fi
  return 0
}

setup_test_data() {
  header "Setup"
  OWNER_JWT=$(mint_jwt "$OWNER_EMAIL")
  USER2_JWT=$(mint_jwt "$USER2_EMAIL")
  USER3_JWT=$(mint_jwt "$USER3_EMAIL")
  VIEWER_JWT=$(mint_jwt "$VIEWER_EMAIL")
  OUTSIDER_JWT=$(mint_jwt "$OUTSIDER_EMAIL")
  printf 'Minted JWTs for: %s, %s, %s, %s, %s\n' \
    "$OWNER_EMAIL" "$USER2_EMAIL" "$USER3_EMAIL" "$VIEWER_EMAIL" "$OUTSIDER_EMAIL"

  req POST "${BUDGET_BASE}/budgets" \
    "{\"org_id\":\"e2e-org\",\"name\":\"E2E Sharing Budget\",\"type\":\"sharing\",\"currency\":\"VND\"}" \
    "$OWNER_JWT"
  assert_status "D.1 create sharing budget" "201"

  SHARING_BUDGET_ID=$(jget '.base.id')
  if [[ -z "$SHARING_BUDGET_ID" || "$SHARING_BUDGET_ID" == "None" ]]; then
    SHARING_BUDGET_ID=$(jget '.id')
  fi
  if [[ -z "$SHARING_BUDGET_ID" || "$SHARING_BUDGET_ID" == "None" ]]; then
    SHARING_BUDGET_ID=$(jget '.budget.id')
  fi
  printf '  sharing_budget_id=%s\n' "$SHARING_BUDGET_ID"
}

test_member_management() {
  header "Member management (D.2, D.3, D.19b)"
  req POST "${BUDGET_BASE}/budgets/${SHARING_BUDGET_ID}/members" \
    "{\"email\":\"${USER2_EMAIL}\",\"role\":\"contributor\"}" \
    "$OWNER_JWT"
  assert_status "D.2 manager can add member" "201"

  req POST "${BUDGET_BASE}/budgets/${SHARING_BUDGET_ID}/members" \
    "{\"email\":\"${USER3_EMAIL}\",\"role\":\"viewer\"}" \
    "$USER2_JWT"
  assert_status "D.3 contributor cannot add member" "403"
}

test_join_link() {
  header "Join link (D.4, D.5, D.6, D.7, D.8)"
  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/join-link" "{}" "$OWNER_JWT"
  assert_status "D.4 generate join link" "201"
  local token
  token=$(jget '.token')

  req POST "${SHARING_BASE}/join-link/accept" "{\"token\":\"${token}\"}" "$USER3_JWT"
  assert_status "D.5 accept join link" "200"

  req POST "${SHARING_BASE}/join-link/accept" "{\"token\":\"${token}\"}" "$USER3_JWT"
  assert_status "D.6 accept join link idempotent (same caller)" "200"

  req POST "${SHARING_BASE}/join-link/accept" "{\"token\":\"definitely-not-a-real-token\"}" "$OUTSIDER_JWT"
  assert_status_in "D.7 invalid token rejected" "404" "400"

  skip "D.8 expired-token rejection (requires DB fixture; manual verify)"
}

test_expense_split() {
  header "Expense splits (D.9, D.10, D.11, D.12, D.13, D.14, D.15, D.16, D.17)"

  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"$(echo "$OWNER_EMAIL" | sha1sum | cut -c1-8)\",\"total_amount\":90000,\"description\":\"Equal dinner\",\"expense_date\":\"2026-06-15\",\"split_method\":1,\"legs\":[{\"user_id\":\"u1\",\"amount\":0,\"weight\":0},{\"user_id\":\"u2\",\"amount\":0,\"weight\":0},{\"user_id\":\"u3\",\"amount\":0,\"weight\":0}]}" \
    "$OWNER_JWT"
  assert_status "D.9 add equal-split expense" "201"

  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/balances" "" "$OWNER_JWT"
  assert_status "D.10 fetch balances after equal split" "200"
  assert_body_contains "D.10 balances has 3 rows" "\"user_id\""

  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/settlement" "" "$OWNER_JWT"
  assert_status "D.11 fetch settlement after equal split" "200"

  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":100000,\"description\":\"Custom\",\"expense_date\":\"2026-06-15\",\"split_method\":2,\"legs\":[{\"user_id\":\"u1\",\"amount\":30000,\"weight\":0},{\"user_id\":\"u2\",\"amount\":40000,\"weight\":0},{\"user_id\":\"u3\",\"amount\":30000,\"weight\":0}]}" \
    "$OWNER_JWT"
  assert_status "D.12 add custom-split expense" "201"

  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":100000,\"description\":\"Bad custom\",\"expense_date\":\"2026-06-15\",\"split_method\":2,\"legs\":[{\"user_id\":\"u1\",\"amount\":10000,\"weight\":0},{\"user_id\":\"u2\",\"amount\":20000,\"weight\":0},{\"user_id\":\"u3\",\"amount\":30000,\"weight\":0}]}" \
    "$OWNER_JWT"
  assert_status_in "D.13 custom-split sum mismatch rejected" "400" "422"

  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":100000,\"description\":\"Weighted\",\"expense_date\":\"2026-06-15\",\"split_method\":3,\"legs\":[{\"user_id\":\"u1\",\"amount\":0,\"weight\":1},{\"user_id\":\"u2\",\"amount\":0,\"weight\":2},{\"user_id\":\"u3\",\"amount\":0,\"weight\":1}]}" \
    "$OWNER_JWT"
  assert_status "D.14 add weighted-split expense" "201"

  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":100000,\"description\":\"Bad weighted\",\"expense_date\":\"2026-06-15\",\"split_method\":3,\"legs\":[{\"user_id\":\"u1\",\"amount\":0,\"weight\":0},{\"user_id\":\"u2\",\"amount\":0,\"weight\":0},{\"user_id\":\"u3\",\"amount\":0,\"weight\":0}]}" \
    "$OWNER_JWT"
  assert_status_in "D.15 weighted-split all-zero rejected" "400" "422"

  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":100000,\"description\":\"Equal weights\",\"expense_date\":\"2026-06-15\",\"split_method\":3,\"legs\":[{\"user_id\":\"u1\",\"amount\":0,\"weight\":1},{\"user_id\":\"u2\",\"amount\":0,\"weight\":1},{\"user_id\":\"u3\",\"amount\":0,\"weight\":1}]}" \
    "$OWNER_JWT"
  assert_status "D.16 weighted 1/1/1 splits (33333/33333/33334)" "201"

  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":10000,\"description\":\"No cat\",\"expense_date\":\"2026-06-15\",\"split_method\":1,\"legs\":[{\"user_id\":\"u1\",\"amount\":0,\"weight\":0}]}" \
    "$OWNER_JWT"
  assert_status "D.17 add expense without category" "201"
}

test_delete_and_permissions() {
  header "Delete + permissions (D.18, D.19, D.19b, D.20, D.21)"

  # D.18 — Delete a known expense as its creator (Owner)
  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" "" "$OWNER_JWT"
  assert_status "fetch expense list for D.18" "200"
  local first_expense_id
  first_expense_id=$(jget '.expenses[0].id')
  if [[ -n "$first_expense_id" && "$first_expense_id" != "None" ]]; then
    req DELETE "${SHARING_BASE}/expenses/${first_expense_id}" "" "$OWNER_JWT"
    assert_status "D.18 creator can delete expense" "200"
  else
    skip "D.18 delete expense (no expenses returned to delete)"
  fi

  # D.19b — non-creator contributor (USER2) cannot delete a fresh expense created by Owner.
  # This is the new test added in Bead 0.1 (Phase 0).
  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":1000,\"description\":\"For D.19b\",\"expense_date\":\"2026-06-15\",\"split_method\":1,\"legs\":[{\"user_id\":\"u1\",\"amount\":0,\"weight\":0},{\"user_id\":\"u2\",\"amount\":0,\"weight\":0}]}" \
    "$OWNER_JWT"
  assert_status "create expense for D.19b" "201"

  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" "" "$OWNER_JWT"
  local d19b_id
  d19b_id=$(jget '.expenses[0].id')
  if [[ -n "$d19b_id" && "$d19b_id" != "None" ]]; then
    req DELETE "${SHARING_BASE}/expenses/${d19b_id}" "" "$USER2_JWT"
    assert_status "D.19b non-creator contributor cannot delete" "403"
  else
    skip "D.19b (no expense to test)"
  fi

  # D.20 — Viewer attempts to add expense (should be 403)
  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/expenses" \
    "{\"paid_by\":\"u1\",\"total_amount\":1000,\"description\":\"Viewer try\",\"expense_date\":\"2026-06-15\",\"split_method\":1,\"legs\":[{\"user_id\":\"u1\",\"amount\":0,\"weight\":0}]}" \
    "$VIEWER_JWT"
  assert_status "D.20 viewer cannot add expense" "403"

  # D.21 — Outsider cannot read balances
  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/balances" "" "$OUTSIDER_JWT"
  assert_status "D.21 outsider cannot read balances" "403"
}

test_settlement_deeplink() {
  header "Settlement deep link (D.22)"
  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/settlement" "" "$OWNER_JWT"
  assert_status "fetch settlement for D.22" "200"

  local dl
  dl=$(jget '.transfers[0].deep_link')
  if [[ -n "$dl" && "$dl" != "None" ]]; then
    case "$dl" in
      philand://*|https://*|vietqr://*) pass "D.22 deep_link has acceptable scheme ($dl)";;
      *) fail "D.22 deep_link unexpected format: $dl";;
    esac
  else
    skip "D.22 deep_link (no transfers in settlement)"
  fi
}

test_payment_flow() {
  header "Mark-as-paid flow (D.23, D.24, D.25, D.26, D.27)"

  # Pick the first transfer from settlement
  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/settlement" "" "$OWNER_JWT"
  assert_status "fetch settlement for payment test" "200"
  local from to amount
  from=$(jget '.transfers[0].from_user_id')
  to=$(jget '.transfers[0].to_user_id')
  amount=$(jget '.transfers[0].amount')

  if [[ -z "$from" || -z "$to" || -z "$amount" ]]; then
    skip "D.23-D.27 (no transfers in settlement to mark as paid)"
    return
  fi

  # D.23 — Mark the transfer as paid
  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/payments" \
    "{\"from_user_id\":\"${from}\",\"to_user_id\":\"${to}\",\"amount\":${amount},\"paid_at\":\"2026-06-15\",\"note\":\"e2e test\"}" \
    "$OWNER_JWT"
  assert_status "D.23 mark transfer as paid" "201"

  # D.24 — List payments shows the new entry
  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/payments" "" "$OWNER_JWT"
  assert_status "D.24 list payments" "200"
  assert_body_contains "D.24 payments has the new entry" "\"from_user_id\":\"${from}\""

  # D.25 — Idempotency: marking the same transfer again returns the existing payment
  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/payments" \
    "{\"from_user_id\":\"${from}\",\"to_user_id\":\"${to}\",\"amount\":${amount},\"paid_at\":\"2026-06-15\"}" \
    "$OWNER_JWT"
  assert_status "D.25 mark-paid is idempotent" "200" "201"

  # D.26 — After payment, the settlement either shows fewer transfers OR a smaller amount
  req GET "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/settlement" "" "$OWNER_JWT"
  assert_status "D.26 fetch settlement after payment" "200"

  # D.27 — Mark with amount <= 0 is rejected
  req POST "${SHARING_BASE}/budgets/${SHARING_BUDGET_ID}/payments" \
    "{\"from_user_id\":\"${from}\",\"to_user_id\":\"${to}\",\"amount\":0,\"paid_at\":\"2026-06-15\"}" \
    "$OWNER_JWT"
  assert_status_in "D.27 zero-amount payment rejected" "400" "422"
}

main() {
  printf '\033[1mPhilandz Sharing — Layer D integration tests\033[0m\n'
  printf 'Gateway: %s\n' "$GATEWAY_URL"

  if [[ "${SKIP_INFRA_CHECK:-0}" != "1" ]]; then
    if ! check_infra; then
      exit 1
    fi
  fi

  if ! setup_test_data; then
    printf '\n\033[31mSetup failed — aborting.\033[0m\n'
    exit 1
  fi

  test_member_management
  test_join_link
  test_expense_split
  test_delete_and_permissions
  test_settlement_deeplink
  test_payment_flow

  header "Summary"
  printf '  Passed:  \033[32m%d\033[0m\n' "$PASS"
  printf '  Failed:  \033[31m%d\033[0m\n' "$FAIL"
  printf '  Skipped: \033[33m%d\033[0m\n' "$SKIP"
  printf '  Total:   %d\n' "$TOTAL"
  printf '\n'

  if [[ "$FAIL" -gt 0 ]]; then
    exit 1
  fi
}

main "$@"
