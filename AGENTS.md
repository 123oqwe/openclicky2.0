# AGENTS.md — OpenClicky 2.0

Read this before changing anything. Full version: `docs/03-agent-playbook.md`.

## Where the truth lives

1. Product behaviour: the source PRD (`docs/00-baseline.md` §1)
2. Technical commitments: `docs/01-tech-prd.md`
3. Decisions and their costs: `docs/adr/` (latest **Accepted**)
4. Module internals: `docs/02-sdd.md`
5. Test expectations: `docs/04-test-design.md`
6. Your own summary: **never** evidence

## Hard rules

- No screen capture unless a user action opened the gate. `capture_all` takes `&CaptureGuard`; there is no other way to call it.
- `crates/core` must not depend on Tauri, GUI, `ureq`, or `std::process`. Checked by `scripts/check-invariants.sh`.
- Platform code only in `crates/platform`. Tools reach the OS only through ports.
- API keys never in code, logs, config, tests or commits. All logging goes through `security::redact`.
- No network in tests, no secrets in fixtures, no `sleep` — inject `Clock`.
- Failing test first, then implementation. Never delete or weaken a test to get green.
- Do not edit shared-write areas without coordination: `crates/core/src/ports.rs`, `crates/core/src/store/migrations.rs`, `docs/01-tech-prd.md`, `docs/02-sdd.md`, `Cargo.toml`, `.github/workflows/`.

## Commands that actually exist

```bash
# always
bash scripts/check-invariants.sh          # structural invariants (docs + code when present)
python3 scripts/doc-audit.py              # document substance check
python3 scripts/criteria-coverage.py      # 88 numbered standard items

# when a Cargo workspace is present (crates/ on your branch)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

There is no frontend yet. `pnpm` commands appear only after `apps/desktop/ui` exists; do not run or cite them before that.

## Branch and PR rules

- Branch from `main`: `feat/*`, `fix/*`, `docs/*`, `chore/*`. Never push to `main`.
- A docs PR must contain only docs: verify with `gh pr view <n> --json files` before merging. (A docs PR once carried 37 code files; it was reverted.)
- PR description: what changed, what did **not** change, dependencies, test output, untested items, risk, rollback.
- "It works" is not evidence. Paste the commands and their output.

## Before you claim done

`check-invariants.sh` + `doc-audit` + `criteria-coverage` must pass; if code is present, fmt/clippy/test must pass. Then fill the PR template. If something is unverified, say so and record it in `docs/06-acceptance.md` §5.

## Commit style

Conventional Commits: `feat|fix|docs|test|chore|refactor(revert is allowed)(scope): subject`.
