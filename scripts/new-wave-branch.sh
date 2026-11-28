#!/usr/bin/env bash
# One branch, one worktree, one writer.
#
# Two agents in the same checkout is how a delegate switched my branch, committed into
# it, and left me rebasing my own work out of its history. A rule in a document did not
# stop that, so the branch creation itself makes the separate directory.
#
#   scripts/new-wave-branch.sh feat/thing            -> ../wt-feat-thing
#   scripts/new-wave-branch.sh feat/thing main       -> from a base other than origin/main
set -eu

branch="${1:?usage: new-wave-branch.sh <branch> [<base>]}"
base="${2:-origin/main}"

root="$(git rev-parse --show-toplevel)"
target="$(dirname "$root")/wt-$(printf '%s' "$branch" | tr '/' '-')"

if [ -e "$target" ]; then
    echo "worktree already at $target" >&2
    exit 1
fi

git -C "$root" fetch -q origin
git -C "$root" worktree add -q -b "$branch" "$target" "$base"

echo "worktree: $target"
echo "cd '$target'"
