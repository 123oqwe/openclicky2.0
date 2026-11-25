#!/usr/bin/env bash
# Proves the ask path on a real binary with no key and no network.
#
#   scripts/e2e-live.sh                    fail unless the screen was captured
#   REQUIRE_SCREEN=0 scripts/e2e-live.sh    tolerate a headless box that cannot capture
#
# The mock provider records every request, so "the screen went to the model" and
# "the prompt carried the annotation grammar" are checked from evidence rather
# than assumed from a green unit test.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${BIN:-$ROOT/target/debug/openclicky}"
PORT="${PORT:-8799}"
REPLY='按钮在右上角 [POINT:120,240:提交按钮:screen0]'

if [ ! -x "$BIN" ]; then
  printf 'no binary at %s; run: cargo build -p openclicky-cli\n' "$BIN" >&2
  exit 2
fi

TMP="$(mktemp -d)"
LOG="$TMP/requests.jsonl"
MOCK_PID=""
cleanup() {
  [ -n "$MOCK_PID" ] && kill "$MOCK_PID" 2>/dev/null
  rm -rf "$TMP"
}
trap cleanup EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

python3 "$ROOT/scripts/mock-provider.py" --port "$PORT" --log "$LOG" --reply "$REPLY" >"$TMP/mock.out" 2>&1 &
MOCK_PID=$!

ready=0
for _ in $(seq 1 100); do
  if curl -sf "http://127.0.0.1:$PORT/v1/models" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.1
done
[ "$ready" = 1 ] || fail "mock provider never came up on port $PORT"

mkdir -p "$TMP/config"
cat >"$TMP/config/config.toml" <<TOML
[provider]
kind = "openai_compatible"
base_url = "http://127.0.0.1:$PORT/v1"
chat_model = "mock-chat"
[voice]
captions = false
TOML

# Older builds have no --remember-file and report a refused keychain as success,
# so try the durable form first and accept the plain one only if it exists.
if ! printf 'test-key\n' | "$BIN" --data-dir "$TMP" set-key --remember-file >/dev/null 2>&1; then
  printf 'test-key\n' | "$BIN" --data-dir "$TMP" set-key >/dev/null 2>&1 || true
fi
# A key that only lived in the set-key process is not a key. Checking the next
# process is what separates "the store kept it" from "set-key said it did".
if ! "$BIN" --data-dir "$TMP" doctor 2>/dev/null | grep -q '^api key: present'; then
  printf 'CANNOT RUN: this build cannot persist a key (no durable store), so ask\n' >&2
  printf 'cannot be exercised. The fix is the durable key store; run again on a\n' >&2
  printf 'build that has it.\n' >&2
  exit 2
fi

OUT="$("$BIN" --data-dir "$TMP" ask --text "提交按钮在哪")" || fail "ask exited non-zero"
printf '%s\n' "$OUT"
case "$OUT" in
  *按钮在右上角*) ;;
  *) fail "the answer text did not come back from the model" ;;
esac

python3 - "$LOG" "${REQUIRE_SCREEN:-1}" <<'PY' || exit 1
import json
import sys

log_path, require_screen = sys.argv[1], sys.argv[2] == "1"
rows = [json.loads(line) for line in open(log_path, encoding="utf-8")]
chats = [r for r in rows if r["kind"] == "chat"]
if not chats:
    print("FAIL: the binary never asked the model", file=sys.stderr)
    sys.exit(1)

body = chats[-1]["body"]
system = body["messages"][0]["content"]
if "POINT:" not in system:
    print("FAIL: the system prompt lost the annotation grammar", file=sys.stderr)
    sys.exit(1)

parts = body["messages"][-1]["content"]
images = [p for p in parts if isinstance(p, dict) and p.get("type") == "image_url"] if isinstance(parts, list) else []
print(f"screens attached: {len(images)}")
if require_screen and not images:
    print(
        "FAIL: no screenshot reached the model. This script exists to prove the "
        "screen is part of the request; set REQUIRE_SCREEN=0 only on a headless box.",
        file=sys.stderr,
    )
    sys.exit(1)

print("model asked:", body.get("model"))
print("OK: answer, prompt and request all arrived")
PY
