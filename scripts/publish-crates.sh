#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF' >&2
usage: scripts/publish-crates.sh [--validate | --publish]

  --validate  Run secret-free manifest validation for the publish graph.
  --publish   Publish workspace crates to crates.io in dependency order.
EOF
  exit 1
}

mode="validate"

if [[ $# -gt 1 ]]; then
  usage
fi

if [[ $# -eq 1 ]]; then
  case "$1" in
    --validate)
      mode="validate"
      ;;
    --publish)
      mode="publish"
      ;;
    *)
      usage
      ;;
  esac
fi

packages=(
  "claude-insight-types"
  "claude-insight-storage"
  "claude-insight-capture"
  "claude-insight-tui"
  "claude-insight-daemon"
  "claude-insight"
)

package_version() {
  cargo pkgid -p "$1" | sed -E 's/.*#.+@//'
}

package_exists_on_crates_io() {
  local package="$1"
  local version="$2"
  local url="https://crates.io/api/v1/crates/${package}/${version}"

  curl --silent --show-error --fail --location "$url" >/dev/null 2>&1
}

wait_for_crates_io_index() {
  local package="$1"
  local version="$2"

  for attempt in $(seq 1 24); do
    if package_exists_on_crates_io "$package" "$version"; then
      return 0
    fi

    sleep 5
    echo "waiting for ${package} ${version} to appear on crates.io (${attempt}/24)" >&2
  done

  echo "timed out waiting for ${package} ${version} to appear on crates.io" >&2
  return 1
}

validate_publish_graph() {
  local final_package="${packages[${#packages[@]}-1]}"
  local final_version
  local tmp_root cargo_home unpacked_root manifest_path

  final_version="$(package_version "$final_package")"
  tmp_root="$(mktemp -d -t claude-insight-publish-validate.XXXXXX)"
  cargo_home="${tmp_root}/cargo-home"
  unpacked_root="${tmp_root}/packages"

  cleanup_validate() {
    rm -rf "$tmp_root"
  }

  trap cleanup_validate RETURN
  mkdir -p "$cargo_home" "$unpacked_root"

  echo "packaging publishable workspace crates for staged validation" >&2
  cargo package --workspace --exclude claude-insight-workspace --locked --allow-dirty --no-verify

  for package in "${packages[@]}"; do
    local version crate_archive
    version="$(package_version "$package")"

    crate_archive="target/package/${package}-${version}.crate"
    tar -xzf "$crate_archive" -C "$unpacked_root"
  done

  {
    echo "[patch.crates-io]"
    for package in "${packages[@]}"; do
      local version
      version="$(package_version "$package")"
      echo "${package} = { path = \"${unpacked_root}/${package}-${version}\" }"
    done
  } > "${cargo_home}/config.toml"

  manifest_path="${unpacked_root}/${final_package}-${final_version}/Cargo.toml"

  CARGO_HOME="$cargo_home" \
  CARGO_TARGET_DIR="${tmp_root}/target" \
  cargo generate-lockfile --manifest-path "$manifest_path"

  echo "building packaged ${final_package} from staged crate sources" >&2
  CARGO_HOME="$cargo_home" \
  CARGO_TARGET_DIR="${tmp_root}/target" \
  cargo build --manifest-path "$manifest_path" --locked
}

publish_workspace() {
  for package in "${packages[@]}"; do
    local version
    version="$(package_version "$package")"

    if package_exists_on_crates_io "$package" "$version"; then
      echo "skipping ${package} ${version}; already visible on crates.io" >&2
      continue
    fi

    echo "publishing ${package} ${version}" >&2
    cargo publish -p "$package" --locked
    wait_for_crates_io_index "$package" "$version"
  done
}

case "$mode" in
  validate)
    validate_publish_graph
    ;;
  publish)
    publish_workspace
    ;;
esac
