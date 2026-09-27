#!/bin/bash
# Runs before an agent reads, edits, or runs a shell command.
# - Reading or writing secrets (.env files, keys, ~/.ssh) is denied.
# - curl, wget, force pushes, and pushes to main are denied.
# - Editing a protected file (the paths in .github/CODEOWNERS) waits for a
#   person to approve.
# Works for Claude Code (PreToolUse) and Cursor (beforeReadFile,
# beforeShellExecution). Needs jq, which ships with macOS 15 and later.
set -euo pipefail

input=$(cat)
event=$(jq -r '.hook_event_name // empty' <<<"$input")
tool=$(jq -r '.tool_name // empty' <<<"$input")
file=$(jq -r '.tool_input.file_path // .tool_input.notebook_path // .tool_input.path // .file_path // empty' <<<"$input")
command=$(jq -r '.tool_input.command // .command // empty' <<<"$input")

root=$(git -C "${CLAUDE_PROJECT_DIR:-$PWD}" rev-parse --show-toplevel 2>/dev/null || echo "${CLAUDE_PROJECT_DIR:-$PWD}")

decide() { # decide deny|ask "reason"
  local decision=$1 reason=$2
  if [[ $event == PreToolUse ]]; then
    jq -n --arg d "$decision" --arg r "$reason" \
      '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: $d, permissionDecisionReason: $r}}'
  else
    jq -n --arg d "$decision" --arg r "$reason" '{permission: $d, userMessage: $r, agentMessage: $r}'
  fi
  exit 0
}

is_secret() {
  local base
  base=$(basename "$1")
  [[ $base == .env || $base == .env.* || $base == *.pem || $base == *.key || $base == *.p12 \
    || $base == id_rsa* || $base == id_ed25519* || $1 == "$HOME/.ssh"* || $1 == */.ssh/* ]]
}

# Protected paths come from CODEOWNERS so the two never drift apart.
protected_match() { # protected_match repo-relative-path
  local rel=$1 pattern
  while read -r pattern _; do
    [[ -z $pattern || $pattern == \#* ]] && continue
    pattern=${pattern#/}
    if [[ $pattern == */ ]]; then
      [[ $rel == "$pattern"* ]] && return 0
    elif [[ $rel == "$pattern" ]]; then
      return 0
    fi
  done <"$root/.github/CODEOWNERS"
  return 1
}

approval="Ask the person you are working with to approve this change, and tell them why it is needed. In a pull request, CODEOWNERS also requires Joshua's review."

if [[ -n $file ]]; then
  [[ $file != /* ]] && file="$root/$file"
  if is_secret "$file"; then
    decide deny "Blocked: $file may hold secrets. Agents do not read or write .env files, keys, or ~/.ssh. If a value is needed, ask the person to provide it."
  fi
  rel=${file#"$root"/}
  if [[ $tool != Read && $event != beforeReadFile && $rel != /* ]] && protected_match "$rel"; then
    decide ask "$rel is protected: it controls what Anchovy can upload or record, which dependencies and models are allowed, or which rules agents follow. $approval"
  fi
fi

if [[ -n $command ]]; then
  if grep -Eq '(^|[;&|[:space:](])(curl|wget)([[:space:]]|$)' <<<"$command"; then
    decide deny "Blocked: agents do not fetch arbitrary addresses with curl or wget. Anchovy's network access is limited to the addresses in privacy/allowed-urls.txt."
  fi
  if grep -Eq '(^|[^[:alnum:]_.-])\.env([^[:alnum:]_-]|$)|\.ssh/|id_(rsa|ed25519)|\.pem([^[:alnum:]]|$)' <<<"$command"; then
    decide deny "Blocked: this command touches a file that may hold secrets (.env, keys, ~/.ssh). Ask the person instead."
  fi
  if grep -Eq 'git[[:space:]]+push' <<<"$command"; then
    if grep -Eq '[[:space:]](-f|--force[[:alnum:]-]*)([[:space:]]|$)|[[:space:]]\+[[:alnum:]]' <<<"$command"; then
      decide deny "Blocked: force pushes are not allowed. Push a new commit instead."
    fi
    if grep -Eq 'git[[:space:]]+push([[:space:]]+[^[:space:]]+)*[[:space:]]+([^[:space:]]*:)?(refs/heads/)?main([[:space:]]|$)' <<<"$command"; then
      decide deny "Blocked: agents never push to main. Push a step branch and open a pull request."
    fi
  fi
  if grep -Eq '(>|[[:space:]]tee[[:space:]]|sed[[:space:]]+-i|perl[[:space:]]+-[[:alnum:]]*i|(^|[[:space:];&|])(mv|cp|rm|ln|chmod)[[:space:]]|git[[:space:]]+(checkout|restore|apply|mv|rm)[[:space:]])' <<<"$command"; then
    while read -r pattern _; do
      [[ -z $pattern || $pattern == \#* ]] && continue
      if [[ $command == *"${pattern#/}"* ]]; then
        decide ask "This command may change the protected path ${pattern#/}. $approval"
      fi
    done <"$root/.github/CODEOWNERS"
  fi
fi

exit 0
