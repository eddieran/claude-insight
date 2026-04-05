#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "usage: $0 <version-tag> <release-dir> [repo]" >&2
  exit 1
fi

version_tag="$1"
release_dir="$2"
repo="${3:-eddieran/claude-insight}"
version="${version_tag#v}"
checksums_file="${release_dir}/SHA256SUMS"

if [[ ! -f "$checksums_file" ]]; then
  echo "missing checksums file: $checksums_file" >&2
  exit 1
fi

sha_for() {
  local asset_name="$1"
  local sha
  sha="$(awk -v asset="$asset_name" '$2 == asset { print $1 }' "$checksums_file")"
  if [[ -z "$sha" ]]; then
    return 1
  fi
  printf '%s' "$sha"
}

asset_url() {
  local asset_name="$1"
  printf 'https://github.com/%s/releases/download/%s/%s' "$repo" "$version_tag" "$asset_name"
}

darwin_arm_asset="claude-insight-${version_tag}-darwin-aarch64.tar.gz"
darwin_intel_asset="claude-insight-${version_tag}-darwin-x86_64.tar.gz"
linux_arm_asset="claude-insight-${version_tag}-linux-aarch64.tar.gz"
linux_intel_asset="claude-insight-${version_tag}-linux-x86_64.tar.gz"

darwin_arm_sha="$(sha_for "$darwin_arm_asset" || true)"
darwin_intel_sha="$(sha_for "$darwin_intel_asset" || true)"
linux_arm_sha="$(sha_for "$linux_arm_asset" || true)"
linux_intel_sha="$(sha_for "$linux_intel_asset" || true)"

emit_platform_block() {
  local platform="$1"
  local arm_asset="$2"
  local arm_sha="$3"
  local intel_asset="$4"
  local intel_sha="$5"
  local cpu_check="$6"

  if [[ -z "$arm_sha" && -z "$intel_sha" ]]; then
    return
  fi

  echo "  on_${platform} do"
  if [[ -n "$arm_sha" && -n "$intel_sha" ]]; then
    echo "    if Hardware::CPU.arm?"
    echo "      url \"$(asset_url "$arm_asset")\""
    echo "      sha256 \"${arm_sha}\""
    echo "    else"
    echo "      url \"$(asset_url "$intel_asset")\""
    echo "      sha256 \"${intel_sha}\""
    echo "    end"
  elif [[ -n "$arm_sha" ]]; then
    echo "    if Hardware::CPU.arm?"
    echo "      url \"$(asset_url "$arm_asset")\""
    echo "      sha256 \"${arm_sha}\""
    echo "    end"
  else
    echo "    if ${cpu_check}"
    echo "      url \"$(asset_url "$intel_asset")\""
    echo "      sha256 \"${intel_sha}\""
    echo "    end"
  fi
  echo "  end"
  echo
}

echo 'class ClaudeInsight < Formula'
echo '  desc "Local observability and audit tooling for Claude Code sessions"'
echo '  homepage "https://github.com/eddieran/claude-insight"'
echo "  version \"${version}\""
echo '  license "MIT"'
echo

emit_platform_block "macos" "$darwin_arm_asset" "$darwin_arm_sha" "$darwin_intel_asset" "$darwin_intel_sha" "Hardware::CPU.intel?"
emit_platform_block "linux" "$linux_arm_asset" "$linux_arm_sha" "$linux_intel_asset" "$linux_intel_sha" "Hardware::CPU.intel?"

cat <<'EOF'
  def install
    bin.install "claude-insight"
  end

  test do
    assert_match "Local observability for Claude Code", shell_output("#{bin}/claude-insight --help")
  end
end
EOF
