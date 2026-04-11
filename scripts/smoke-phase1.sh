#!/usr/bin/env bash
set -euo pipefail

script_dir="$(dirname "$0")"
project_root="$(cd "$script_dir/.." && pwd)"
cd "$project_root"

base_url="${AISIX_BASE_URL:-http://127.0.0.1:4000}"
admin_key="${AISIX_ADMIN_KEY:-change-me-admin-key}"
provider_id="${AISIX_PROVIDER_ID:-openai}"
provider_base_url="${AISIX_PROVIDER_BASE_URL:-https://api.openai.com}"
provider_secret_ref="${AISIX_PROVIDER_SECRET_REF:-env:OPENAI_API_KEY}"
upstream_model="${AISIX_UPSTREAM_MODEL:-gpt-4o-mini}"
virtual_key="${AISIX_VIRTUAL_KEY:-sk-smoke-phase1}"
reload_wait_seconds="${AISIX_RELOAD_WAIT_SECONDS:-20}"

curl -fsS "$base_url/health" >/dev/null
curl -fsS "$base_url/ready" >/dev/null

curl -fsS -X PUT "$base_url/admin/providers/$provider_id" \
  -H 'content-type: application/json' \
  -H "x-admin-key: $admin_key" \
  -d "$(cat <<EOF
{
  "id": "$provider_id",
  "kind": "openai",
  "base_url": "$provider_base_url",
  "auth": {"secret_ref": "$provider_secret_ref"},
  "policy_id": null,
  "rate_limit": null
}
EOF
)" >/dev/null

curl -fsS -X PUT "$base_url/admin/models/$upstream_model" \
  -H 'content-type: application/json' \
  -H "x-admin-key: $admin_key" \
  -d "$(cat <<EOF
{
  "id": "$upstream_model",
  "provider_id": "$provider_id",
  "upstream_model": "$upstream_model",
  "policy_id": null,
  "rate_limit": null
}
EOF
)" >/dev/null

curl -fsS -X PUT "$base_url/admin/apikeys/smoke-key" \
  -H 'content-type: application/json' \
  -H "x-admin-key: $admin_key" \
  -d "$(cat <<EOF
{
  "id": "smoke-key",
  "key": "$virtual_key",
  "allowed_models": ["$upstream_model"],
  "policy_id": null,
  "rate_limit": null
}
EOF
)" >/dev/null

chat_payload="$(cat <<EOF
{
  "model": "$upstream_model",
  "messages": [{"role": "user", "content": "Say hello."}],
  "stream": false
}
EOF
)"

chat_ok=0
for _ in $(seq 1 "$reload_wait_seconds"); do
  response="$(curl -sS -o /dev/null -w '%{http_code}' "$base_url/v1/chat/completions" \
    -H 'content-type: application/json' \
    -H "authorization: Bearer $virtual_key" \
    -d "$chat_payload" || true)"

  if [ "$response" = "200" ]; then
    chat_ok=1
    break
  fi

  sleep 1
done

if [ "$chat_ok" -ne 1 ]; then
  printf 'gateway chat did not succeed after waiting for watcher reload\n' >&2
  exit 1
fi

printf 'phase1 smoke verified health, admin write, and gateway chat\n'
