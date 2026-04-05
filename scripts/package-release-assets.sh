#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 3 ]]; then
  echo "usage: $0 <version-tag> [raw-artifact-dir] [release-dir]" >&2
  exit 1
fi

version_tag="$1"
raw_dir="${2:-dist/raw}"
release_dir="${3:-dist/release}"
bin_name="claude-insight"
release_dir_abs="$(mkdir -p "$release_dir" && cd "$release_dir" && pwd)"

checksum_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1"
  else
    shasum -a 256 "$1"
  fi
}

rm -rf "$release_dir_abs"
mkdir -p "$release_dir_abs"

while IFS= read -r -d '' binary; do
  artifact_name="$(basename "$(dirname "$binary")")"
  binary_name="$(basename "$binary")"

  case "$artifact_name" in
    windows-*)
      asset_name="${bin_name}-${version_tag}-${artifact_name}.zip"
      (
        cd "$(dirname "$binary")"
        zip -q -j "${release_dir_abs}/${asset_name}" "$binary_name"
      )
      ;;
    *)
      asset_name="${bin_name}-${version_tag}-${artifact_name}.tar.gz"
      chmod +x "$binary"
      tar -czf "${release_dir_abs}/${asset_name}" -C "$(dirname "$binary")" "$binary_name"
      ;;
  esac
done < <(find "$raw_dir" -type f -name "${bin_name}*" -print0)

(
  cd "$release_dir_abs"
  : > SHA256SUMS
  for asset in ./*; do
    [[ "$(basename "$asset")" == "SHA256SUMS" ]] && continue
    checksum_file "$(basename "$asset")" >> SHA256SUMS
  done
)

"$(dirname "$0")/generate-homebrew-formula.sh" "$version_tag" "$release_dir_abs" \
  > "${release_dir_abs}/claude-insight.rb"
