#!/usr/bin/env bash
# A closing keyword plus an issue reference in a pull request body closes that
# issue on merge. The machine cannot tell a real fix from an unverified one, and
# it has guessed wrong three times in one night: the issue was closed by the
# wording of a description while nothing had ever been run against the promise
# inside it.
#
# So the rule is evidence first: before the body may close #N, #N carries at
# least one comment, and that comment is where the command and its output go.
# No evidence, no keyword -- write `Refs #N` instead.
#
# Commit bodies are a separate surface, already covered by the history rule in
# scripts/check-invariants.sh: a squash merge copies them into main.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

# GitHub accepts nine keyword forms, each followed by the reference:
# close, closes, closed, fix, fixes, fixed, resolve, resolves, resolved.
# Source: docs.github.com, "Using keywords in issues and pull requests".
#
# The first version of this guard knew three of them (closes, fixes, resolves),
# so `Close #117`, `Fixed #117` and `Resolved #117` walked past it and would have
# closed an issue with nothing in it. `--self-test` holds all nine.
#
# Known gaps, neither of which appears in the documentation: a colon separator
# (`Closes: #10`) and the cross-repository reference form
# (`Fixes owner/repo#100`, which closes over there, not here).
CLOSE_FORMS='close[sd]?|fix(e[sd])?|resolve[sd]?'

usage() {
  echo "usage: check-close-evidence.sh <pr-number>" >&2
  echo "       check-close-evidence.sh --text '<pr body>'" >&2
  echo "       check-close-evidence.sh --self-test" >&2
  exit 2
}

# The issue numbers a body would close, once each.
close_targets() {
  printf '%s\n' "$1" \
    | grep -Eio "($CLOSE_FORMS)[[:space:]]+#[0-9]+" \
    | grep -Eo '[0-9]+' \
    | sort -un
}

# The three things the guard says. None of them puts a keyword form directly
# before a number: `close #117` is itself a closing keyword, so a message written
# the obvious way cannot be quoted in the pull request it refused. Twice it could
# not. self_test asserts these three stay clean, which means defining them before
# the dispatch rather than beside the loop that prints them.
note_allowed()     { echo "ok: #$1 carries $2 comment(s), so closing it is allowed"; }
note_unreadable()  { echo "::error::#$1 cannot be read (missing, or invisible to this token), so this body cannot be trusted to close it. Use Refs #$1, or open the issue first."; }
note_no_evidence() { echo "::error::#$1 would be closed by this body on merge, and #$1 has no comment at all. An issue needs an evidence comment first: paste the command you ran and its output into it. Otherwise write Refs #$1 and let a human decide."; }

# A guard that cannot match proves nothing, so this asserts the whole keyword
# table instead of the three forms whoever wrote the regular expression
# happened to remember. No network, no gh.
self_test() {
  fail=0
  for word in close closes closed fix fixes fixed resolve resolves resolved; do
    targets=$(close_targets "the body says $word #42")
    if [ "$targets" = "42" ]; then
      echo "ok   form=$word target=42"
    else
      echo "FAIL form=$word matched nothing, and GitHub reads it as closing intent"
      fail=1
    fi
  done
  while IFS= read -r body; do
    targets=$(close_targets "$body")
    if [ -z "$targets" ]; then
      echo "ok   no match for: $body"
    else
      echo "FAIL matched $targets in: $body"
      fail=1
    fi
  done <<'CASES'
Refs #42
closing #42
closes #forty-two
CASES
  for own in "$(note_allowed 42 1)" "$(note_unreadable 42)" "$(note_no_evidence 42)"; do
    targets=$(close_targets "$own")
    # An empty message passes close_targets, so an empty one would make this
    # check green while proving nothing.
    if [ -z "$own" ]; then
      echo "FAIL a message the guard prints came out empty, so the check below is vacuous"
      fail=1
    elif [ -z "$targets" ]; then
      echo "ok   own output is pasteable: $own"
    else
      echo "FAIL a body quoting the guard would be blocked by it: $own"
      fail=1
    fi
  done
  if [ "$fail" -ne 0 ]; then
    echo "close-evidence self-test: FAILED"
    exit 1
  fi
  echo "close-evidence self-test: OK"
}

case "${1:-}" in
  --self-test)
    self_test
    exit 0
    ;;
  --text)
    body="${2:-}"
    ;;
  "")
    usage
    ;;
  *)
    body=$(gh pr view "$1" --json body --jq '.body // ""') || {
      echo "::error::cannot read the body of pull request #$1"
      exit 1
    }
    ;;
esac

numbers=$(close_targets "$body")

if [ -z "$numbers" ]; then
  echo "no auto-close keyword in the body, nothing to check"
  exit 0
fi

# The three things the guard says. These are printed by the loop below.

fail=0
for n in $numbers; do
  count=$(gh issue view "$n" --json comments --jq '.comments | length' 2>/dev/null) || count=""
  if [ -z "$count" ]; then
    note_unreadable "$n"
    fail=1
  elif [ "$count" -eq 0 ]; then
    note_no_evidence "$n"
    fail=1
  else
    note_allowed "$n" "$count"
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "close-evidence: FAILED"
  exit 1
fi
echo "close-evidence: OK"
