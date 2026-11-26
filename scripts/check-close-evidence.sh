#!/usr/bin/env bash
# A `Closes #N` (also Fixes, Resolves) in a pull request body closes that issue
# on merge. The machine cannot tell a real fix from an unverified one, and it has
# guessed wrong three times in one night: the issue was closed by the wording of
# a description while nothing had ever been run against the promise inside it.
#
# So the rule is evidence first: before the body may close #N, #N carries at
# least one comment, and that comment is where the command and its output go.
# No evidence, no Closes -- write `Refs #N` instead.
#
# Commit bodies are a separate surface, already covered by the history rule in
# scripts/check-invariants.sh: a squash merge copies them into main.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

usage() {
  echo "usage: check-close-evidence.sh <pr-number>"
  echo "       check-close-evidence.sh --text '<pr body>'" >&2
  exit 2
}

if [ "${1:-}" = "--text" ]; then
  body="${2:-}"
else
  pr="${1:-${PR_NUMBER:-}}"
  [ -n "$pr" ] || usage
  body=$(gh pr view "$pr" --json body --jq '.body // ""') || {
    echo "::error::cannot read the body of pull request #$pr"
    exit 1
  }
fi

# `sort -un` so two mentions of the same issue are checked once.
numbers=$(printf '%s\n' "$body" \
  | grep -Eio '(closes|fixes|resolves)[[:space:]]+#[0-9]+' \
  | grep -Eo '[0-9]+' \
  | sort -un)

if [ -z "$numbers" ]; then
  echo "no auto-close keyword in the body, nothing to check"
  exit 0
fi

fail=0
for n in $numbers; do
  count=$(gh issue view "$n" --json comments --jq '.comments | length' 2>/dev/null) || count=""
  # No keyword next to a number in these messages, on purpose: the guard's own
  # output has to be pasteable into the pull request body it just refused.
  if [ -z "$count" ]; then
    echo "::error::the body asks GitHub to close #$n, but #$n cannot be read (missing, or invisible to this token). That is not evidence either. Use Refs #$n, or open the issue first."
    fail=1
  elif [ "$count" -eq 0 ]; then
    echo "::error::the body asks GitHub to close #$n on merge, and #$n has no comment at all. An issue needs an evidence comment first: paste the command you ran and its output into #$n. Otherwise write Refs #$n and let a human decide."
    fail=1
  else
    echo "ok: #$n carries $count comment(s), so closing it is allowed"
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "close-evidence: FAILED"
  exit 1
fi
echo "close-evidence: OK"
