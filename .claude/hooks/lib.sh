# Shared by the hook scripts. Source it after setting $root to the
# repository's top level. Written for the bash 3.2 that ships with macOS.

# repo_path PATH: prints PATH relative to $root the way the file system sees
# it, so "src/../AGENTS.md", "./AGENTS.md", and a symlink into the repository
# all name the same file. Resolves "." and "..", then symlinks in the deepest
# part of the path that exists. Prints an absolute path when PATH is outside
# the repository.
repo_path() {
  local path=$1 part rest="" real_root
  local -a parts out=()
  [[ $path != /* ]] && path="$root/$path"

  IFS=/ read -ra parts <<<"$path"
  for part in ${parts[@]+"${parts[@]}"}; do
    case $part in
      "" | .) ;;
      ..) ((${#out[@]})) && out=(${out[@]+"${out[@]:0:${#out[@]}-1}"}) ;;
      *) out+=("$part") ;;
    esac
  done
  path="/$(IFS=/; echo "${out[*]-}")"

  # A file being created does not exist yet; resolve its nearest existing parent.
  while [[ ! -e $path && $path != / ]]; do
    rest="/${path##*/}$rest"
    path=${path%/*}
    [[ -z $path ]] && path=/
  done
  path=$(realpath "$path")$rest
  path=${path//\/\//\/}

  real_root=$(realpath "$root")
  if [[ $path == "$real_root" ]]; then
    echo .
  else
    echo "${path#"$real_root"/}"
  fi
}
