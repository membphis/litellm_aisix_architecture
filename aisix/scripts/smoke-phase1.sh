#!/usr/bin/env bash
set -euo pipefail

script_dir="$(dirname "$0")"
project_root="$(cd "$script_dir/.." && pwd)"
cd "$project_root"

base_url="${AISIX_BASE_URL:-http://127.0.0.1:4000}"
admin_key="${AISIX_ADMIN_KEY:-change-me-admin-key}"
provider_base_url="${AISIX_PROVIDER_BASE_URL:-https://api.openai.com}"
virtual_key="${AISIX_VIRTUAL_KEY:-sk-smoke-phase1}"

curl -fsS "$base_url/health" >/dev/null
curl -fsS "$base_url/ready" >/dev/null

curl -fsS -X PUT "$base_url/admin/providers/openai" \
  -H 'content-type: application/json' \
  -H "x-admin-key: $admin_key" \
  -d "$(cat <<EOF
{
  \"id\": \"openai\",
  \"kind\": \"openai\",
  \"base_url\": \"$provider_base_url\",
  \"auth\": {\"secret_ref\": \"env:OPENAI_API_KEY\"},
  \"policy_id\": null,
  \"rate_limit\": null
}
EOF
)" >/dev/null

curl -fsS -X PUT "$base_url/admin/models/gpt-4o-mini" \
  -H 'content-type: application/json' \
  -H "x-admin-key: $admin_key" \
  -d '{
    "id": "gpt-4o-mini",
    "provider_id": "openai",
    "upstream_model": "gpt-4o-mini",
    "policy_id": null,
    "rate_limit": null
  }' >/dev/null

curl -fsS -X PUT "$base_url/admin/apikeys/smoke-key" \
  -H 'content-type: application/json' \
  -H "x-admin-key: $admin_key" \
  -d "$(cat <<EOF
{
  \"id\": \"smoke-key\",
  \"key\": \"$virtual_key\",
  \"allowed_models\": [\"gpt-4o-mini\"],
  \"policy_id\": null,
  \"rate_limit\": null
}
EOF
)" >/dev/null

curl -fsS "$base_url/v1/chat/completions" \
  -H 'content-type: application/json' \
  -H "authorization: Bearer $virtual_key" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Say hello."}],
    "stream": false
  }' >/dev/null

printf 'phase1 smoke verified health, admin write, and gateway chat\n'
