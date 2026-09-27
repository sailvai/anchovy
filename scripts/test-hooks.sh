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
raw_file() { # raw_file label expected tool path-as-given
  decision "$2" "$(jq -n --arg t "$3" --arg f "$4" '{hook_event_name: "PreToolUse", tool_name: $t, tool_input: {file_path: $f}}')" "$1"
}
file() { # file label expected tool repo-path
  raw_file "$1" "$2" "$3" "$PWD/$4"
}

links=$(mktemp -d)
trap 'rm -rf "$links"' EXIT
ln -s "$PWD/src-tauri" "$links/src-tauri"
ln -s "$PWD/AGENTS.md" "$links/rules.md"

shell "push branch, then gh --base main" allow 'git push -u origin step-00 2>&1 | tail -2; gh pr create --base main'
shell "push to main" deny 'git push origin main'
shell "push HEAD:main" deny 'git push origin HEAD:main'
shell "push HEAD:refs/heads/main" deny 'cd x && git push origin HEAD:refs/heads/main'
shell "force flag after the branch" deny 'git push origin step-01 --force'
shell "force -f" deny 'git push -f origin step-01'
shell "plus refspec" deny 'git push origin +step-01'
shell "force inside -fu" deny 'git push -fu origin step-01'
shell "force inside -uf" deny 'git push -uf origin step-01'
shell "force inside -qfu after the branch" deny 'git push origin step-01 -qfu'
shell "force-with-lease=ref" deny 'git push --force-with-lease=step-01 origin step-01'
shell "force-if-includes" deny 'git push --force-if-includes origin step-01'
shell "force with git -C before push" deny 'git -C . push -f origin step-01'
shell "main with git -c before push" deny 'git -c push.default=current push origin main'
shell "set upstream with -u" allow 'git push -u origin step-01'
shell "follow tags" allow 'git push --follow-tags origin step-01'
shell "dry run -nu" allow 'git push -nu origin step-01'
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

# Different spellings of a protected path must still ask.
file "protected path through .." ask Edit src/../src-tauri/Entitlements.plist
file "protected path through ./" ask Edit ./AGENTS.md
file "new protected file through .." ask Write src/app/../../.claude/skills/new/SKILL.md
raw_file "relative protected path" ask Edit src-tauri/./capabilities/default.json
file "protected path in other case" ask Edit src-tauri/ENTITLEMENTS.PLIST
file "protected folder in other case" ask Write .Claude/settings.json
raw_file "symlinked folder into the repo" ask Edit "$links/src-tauri/tauri.conf.json"
raw_file "symlink to a protected file" ask Edit "$links/rules.md"
file "unprotected path through .." allow Edit src-tauri/../src/app/App.tsx
shell "sed after cd" ask 'cd src-tauri && sed -i "" s/a/b/ Entitlements.plist'
shell "rm a protected folder" ask 'rm -rf .claude'
shell "redirect from check-privacy.mjs" allow 'node scripts/check-privacy.mjs > log.txt'

decision deny "$(jq -n --arg f "$PWD/.env" '{hook_event_name: "beforeReadFile", file_path: $f}')" "Cursor: read a secrets file"
decision deny "$(jq -n '{hook_event_name: "beforeShellExecution", command: "wget example.com"}')" "Cursor: wget"

if ((failures)); then
  echo "Hook tests failed: $failures"
  exit 1
fi
echo "Hook tests passed"
