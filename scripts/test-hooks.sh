#!/bin/bash
# Feeds sample inputs to .claude/hooks/protect-files.sh and checks each
# decision. Part of `npm run verify`. Exits non-zero on any wrong decision.
cd "$(dirname "$0")/.." || exit 1
export CLAUDE_PROJECT_DIR=$PWD

failures=0
decision() { # decision expected hook-input label
  local got
  got=$(.claude/hooks/protect-files.sh <<<"$2" | jq -r '.hookSpecificOutput.permissionDecision // .permission')
  [[ -z $got ]] && got=allow
  if [[ $got == "$1" ]]; then
    printf 'ok    %-40s %s\n' "$3" "$got"
  else
    printf 'WRONG %-40s expected %s, got %s\n' "$3" "$1" "$got"
    failures=$((failures + 1))
  fi
}
shell() { # shell label expected command
  decision "$2" "$(jq -n --arg c "$3" '{hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: {command: $c}}')" "$1"
}
file() { # file label expected tool path
  decision "$2" "$(jq -n --arg t "$3" --arg f "$PWD/$4" '{hook_event_name: "PreToolUse", tool_name: $t, tool_input: {file_path: $f}}')" "$1"
}

shell "push branch, then gh --base main" allow 'git push -u origin step-00 2>&1 | tail -2; gh pr create --base main'
shell "push to main" deny 'git push origin main'
shell "push HEAD:main" deny 'git push origin HEAD:main'
shell "push HEAD:refs/heads/main" deny 'cd x && git push origin HEAD:refs/heads/main'
shell "force flag after the branch" deny 'git push origin step-01 --force'
shell "force -f" deny 'git push -f origin step-01'
shell "plus refspec" deny 'git push origin +step-01'
shell "branch named main-fix" allow 'git push origin main-fix'
shell "plain push" allow 'git push'
shell "cat a secrets file" deny 'cat .env'
shell "curl" deny 'curl example.com'
shell "process.env in a search" allow 'grep -r process.env src'
shell "sed on tauri.conf.json" ask 'sed -i "" s/a/b/ src-tauri/tauri.conf.json'
shell "cat AGENTS.md" allow 'cat AGENTS.md'

file "edit Entitlements.plist" ask Edit src-tauri/Entitlements.plist
file "write a skill" ask Write .claude/skills/example/SKILL.md
file "edit the address allowlist" ask Edit privacy/allowed-urls.txt
file "edit App.tsx" allow Edit src/app/App.tsx
file "read AGENTS.md" allow Read AGENTS.md
file "read a secrets file" deny Read .env
file "read a local secrets file" deny Read .env.local

decision deny "$(jq -n --arg f "$PWD/.env" '{hook_event_name: "beforeReadFile", file_path: $f}')" "Cursor: read a secrets file"
decision deny "$(jq -n '{hook_event_name: "beforeShellExecution", command: "wget example.com"}')" "Cursor: wget"

if ((failures)); then
  echo "Hook tests failed: $failures"
  exit 1
fi
echo "Hook tests passed"
