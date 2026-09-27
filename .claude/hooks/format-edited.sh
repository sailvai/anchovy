#!/bin/bash
# Runs after an agent edits a file and formats only that file.
# Never blocks: a formatting failure shows up later in `npm run verify`.
input=$(cat)
file=$(jq -r '.tool_input.file_path // .tool_input.notebook_path // .file_path // empty' <<<"$input")
[[ -z $file ]] && exit 0

root=$(git -C "${CLAUDE_PROJECT_DIR:-$PWD}" rev-parse --show-toplevel 2>/dev/null || echo "${CLAUDE_PROJECT_DIR:-$PWD}")
[[ $file != /* ]] && file="$root/$file"
[[ -f $file ]] || exit 0

case $file in
  *.rs)
    rustfmt --edition 2021 --config-path "$root/src-tauri/rustfmt.toml" "$file" >/dev/null 2>&1 ;;
  *.ts | *.tsx | *.js | *.mjs | *.json | *.css | *.html | *.md | *.yml | *.yaml)
    [[ -x $root/node_modules/.bin/prettier ]] &&
      (cd "$root" && node_modules/.bin/prettier --write --ignore-unknown --log-level silent "$file") ;;
esac
exit 0
