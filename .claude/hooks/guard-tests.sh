#!/bin/bash
# Runs before an agent edits a file. On bug-fix branches (fix-*), the test
# that reproduces the bug is committed first and must not change while the
# fix is written. So editing a test file that already exists in HEAD waits
# for a person to approve. New test files are allowed.
# Rust tests inside source files (#[cfg(test)]) are not covered; reviewers
# check those.
set -euo pipefail

input=$(cat)
file=$(jq -r '.tool_input.file_path // .tool_input.notebook_path // .file_path // empty' <<<"$input")
[[ -z $file ]] && exit 0

root=$(git -C "${CLAUDE_PROJECT_DIR:-$PWD}" rev-parse --show-toplevel 2>/dev/null) || exit 0
branch=$(git -C "$root" branch --show-current 2>/dev/null || true)
[[ $branch == fix-* ]] || exit 0

[[ $file != /* ]] && file="$root/$file"
rel=${file#"$root"/}
case $rel in
  *.test.ts | *.test.tsx | *.spec.ts | tests/* | src-tauri/tests/*) ;;
  *) exit 0 ;;
esac
git -C "$root" cat-file -e "HEAD:$rel" 2>/dev/null || exit 0

reason="$rel is a committed test, and $branch is a bug-fix branch. Fix the code, not the test. If the test itself is wrong, ask the person to approve the change and say why."
jq -n --arg r "$reason" \
  '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "ask", permissionDecisionReason: $r}}'
