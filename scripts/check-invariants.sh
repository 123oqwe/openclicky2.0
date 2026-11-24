#!/usr/bin/env bash
# Structural invariants that must hold regardless of who wrote the code.
# Runs on every PR and locally before "done". Exit non-zero on any violation.
set -uo pipefail

fail=0
note() { printf '%s\n' "$*"; }
bad()  { printf 'VIOLATION: %s\n' "$*"; fail=1; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

has_code=0
[ -f Cargo.toml ] && has_code=1

# ---------------------------------------------------------------- docs
# Every file path referenced by an ADR or by the baseline must exist, so that
# "see 06-acceptance.md" never points at nothing.
note "== docs: referenced deliverables exist =="
for f in docs/00-baseline.md docs/01-tech-prd.md docs/02-sdd.md docs/03-agent-playbook.md docs/04-test-design.md; do
  [ -f "$f" ] || bad "missing required document $f"
done
for p in docs/05-deployment.md docs/06-acceptance.md; do
  [ -f "$p" ] || bad "missing referenced deliverable $p"
done

note "== docs: numbering and coverage =="
python3 scripts/doc-audit.py >/dev/null || bad "doc-audit failed"
python3 scripts/criteria-coverage.py >/dev/null || bad "criteria-coverage failed"
python3 scripts/ref-check.py >/dev/null || bad "broken cross-reference (section, ADR id or path)"

note "== tds: matrix must match the real tests =="
if ! python3 scripts/tds-evidence.py >/dev/null; then
  bad "tds-evidence failed (a cited test does not exist)"
fi
if ! git diff --quiet -- docs/04-test-design.md; then
  bad "docs/04-test-design.md matrix is stale; run scripts/tds-evidence.py and commit"
fi

# ---------------------------------------------------------------- secrets
note "== secrets: no key-shaped literals in the repo =="
# A real key is >= 20 chars of printable non-space after a known prefix.
if command -v rg >/dev/null 2>&1; then
  hits=$(rg -n --hidden -g '!*.json' -g '!docs/**' -g '!.git/**' \
        '(sk-[A-Za-z0-9]{20,}|xox[bap]-[A-Za-z0-9-]{10,}|ghp_[A-Za-z0-9]{20,})' . || true)
else
  hits=$(grep -rnE '(sk-[A-Za-z0-9]{20,}|xox[bap]-[A-Za-z0-9-]{10,}|ghp_[A-Za-z0-9]{20,})' \
        --exclude-dir=.git --exclude-dir=docs --exclude='*.json' . || true)
fi
[ -z "$hits" ] || bad "possible credential literal: $hits"

# ---------------------------------------------------------------- code
if [ "$has_code" -eq 1 ]; then
  note "== core: no GUI / HTTP / process dependencies =="
  core_manifest="crates/core/Cargo.toml"
  if [ -f "$core_manifest" ]; then
    for banned in tauri reqwest ureq tokio hyper; do
      if grep -qE "^${banned}[[:space:]]*=" "$core_manifest"; then
        bad "crates/core depends on $banned (must stay pure)"
      fi
    done
  fi

  note "== core: no direct std::process usage =="
  if grep -rn --include='*.rs' 'std::process' crates/core/src 2>/dev/null | grep -v '^$' >/dev/null; then
    bad "crates/core/src uses std::process (route it through CommandRunner)"
  fi

  note "== capture: only one call site, and it passes a guard =="
  calls=$(grep -rn --include='*.rs' 'capture_all' crates/core/src crates/platform/src apps 2>/dev/null \
          | grep -v 'fn capture_all' | grep -v '^.*://' || true)
  if [ -n "$calls" ]; then
    # Every production call must pass a guard argument (contains '(' and a variable),
    # and must be inside voice.rs (the only authorised orchestration point).
    echo "$calls" | while read -r line; do
      case "$line" in
        *crates/core/src/voice.rs*|*crates/platform/src/*) ;;
        *) echo "VIOLATION: capture outside voice.rs: $line" ;;
      esac
    done
    if echo "$calls" | grep -vE 'capture_all\((&|guard)' >/dev/null; then
      bad "capture_all called without a guard argument"
    fi
  fi

  note "== permissions: probe before tool execution =="
  if ! grep -rn --include='*.rs' 'permission_probe\|PermissionProbe' crates/core/src >/dev/null 2>&1; then
    bad "no PermissionProbe port wired in core"
  fi
else
  note "== code invariants skipped: no Cargo.toml in this tree =="
fi

if [ "$fail" -ne 0 ]; then
  echo "invariants: FAILED"
  exit 1
fi
echo "invariants: OK"
