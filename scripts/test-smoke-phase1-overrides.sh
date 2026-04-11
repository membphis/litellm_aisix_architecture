#!/usr/bin/env bash
set -euo pipefail

script_dir="$(dirname "$0")"
project_root="$(cd "$script_dir/.." && pwd)"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

cat > "$tmp_dir/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

out_dir="${FAKE_CURL_OUT_DIR:?}"
state_dir="${FAKE_CURL_STATE_DIR:?}"
index_file="$out_dir/index"
if [ ! -f "$index_file" ]; then
  printf '0' > "$index_file"
fi
index="$(cat "$index_file")"
printf '%s' "$((index + 1))" > "$index_file"

printf '%s\n' "$*" > "$out_dir/request_${index}.txt"

case "$*" in
  *"/health"*)
    printf '200'
    exit 0
    ;;
  *"/ready"*)
    printf '200'
    exit 0
    ;;
  *"/admin/providers/"*|*"/admin/models/"*|*"/admin/apikeys/"*)
    printf '%s' "$(( $(cat "$state_dir/admin_count" 2>/dev/null || printf '0') + 1 ))" > "$state_dir/admin_count"
    printf '200'
    exit 0
    ;;
  *"/v1/chat/completions"*)
    admin_count="$(cat "$state_dir/admin_count" 2>/dev/null || printf '0')"
    chat_attempts="$(( $(cat "$state_dir/chat_attempts" 2>/dev/null || printf '0') + 1 ))"
    printf '%s' "$chat_attempts" > "$state_dir/chat_attempts"

    if [ "$admin_count" -lt 3 ] || [ "$chat_attempts" -lt 2 ]; then
      printf 'watcher not ready\n' >&2
      printf '400'
      exit 22
    fi

    printf '200'
    exit 0
    ;;
esac

printf '200'
exit 0
EOF
chmod +x "$tmp_dir/curl"
mkdir -p "$tmp_dir/state"

PATH="$tmp_dir:$PATH" \
FAKE_CURL_OUT_DIR="$tmp_dir" \
FAKE_CURL_STATE_DIR="$tmp_dir/state" \
AISIX_PROVIDER_BASE_URL="https://api.deepseek.com" \
AISIX_PROVIDER_SECRET_REF="env:DEEPSEEK_API_KEY" \
AISIX_UPSTREAM_MODEL="deepseek-chat" \
bash "$project_root/scripts/smoke-phase1.sh"

grep -Fq '"base_url": "https://api.deepseek.com"' "$tmp_dir/request_2.txt"
grep -Fq '"secret_ref": "env:DEEPSEEK_API_KEY"' "$tmp_dir/request_2.txt"
grep -Fq '"id": "deepseek-chat"' "$tmp_dir/request_3.txt"
grep -Fq '"upstream_model": "deepseek-chat"' "$tmp_dir/request_3.txt"
grep -Fq '"model": "deepseek-chat"' "$tmp_dir/request_5.txt"
grep -Fq '/v1/chat/completions' "$tmp_dir/request_6.txt"

printf 'smoke overrides verified\n'
