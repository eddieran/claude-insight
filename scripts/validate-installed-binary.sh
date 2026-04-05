#!/usr/bin/env bash
set -euo pipefail

artifact_dir="dist/release"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --artifact-dir)
      artifact_dir="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

resolve_asset_name() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux) os="linux" ;;
    Darwin) os="darwin" ;;
    *)
      echo "unsupported host OS: $os" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *)
      echo "unsupported host architecture: $arch" >&2
      exit 1
      ;;
  esac

  local asset
  asset="$(find "$artifact_dir" -maxdepth 1 -type f -name "claude-insight-v*-${os}-${arch}.tar.gz" | head -n 1)"
  if [[ -z "$asset" ]]; then
    echo "no packaged asset found for ${os}-${arch} in ${artifact_dir}" >&2
    exit 1
  fi
  printf '%s' "$asset"
}

resolve_expected_version() {
  local asset_name="$1"
  local version
  version="$(basename "$asset_name" | sed -E 's/^claude-insight-v([0-9]+\.[0-9]+\.[0-9]+)-.*$/\1/')"
  if [[ -z "$version" || "$version" == "$(basename "$asset_name")" ]]; then
    echo "failed to derive version from asset name: $asset_name" >&2
    exit 1
  fi
  printf '%s' "$version"
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "expected output to contain: $needle" >&2
    echo "$haystack" >&2
    exit 1
  fi
}

rewrite_fixture() {
  local file="$1"
  local session_id="$2"
  local workspace_dir="$3"
  sed \
    -e "s#1a278366-a037-43a6-88e3-f85854ab34f1#${session_id}#g" \
    -e "s#/workspace/redacted/AgentInsight#${workspace_dir}#g" \
    -e "s#/workspace/.claude/projects/claude-insight/11111111-1111-4111-8111-111111111111.jsonl#${workspace_dir}/.claude/projects/claude-insight/${session_id}.jsonl#g" \
    -e "s#11111111-1111-4111-8111-111111111111#${session_id}#g" \
    "$file"
}

asset_path="$(resolve_asset_name)"
expected_version="$(resolve_expected_version "$asset_path")"
tmp_root="$(mktemp -d -t claude-insight-installed-smoke.XXXXXX)"
install_root="${tmp_root}/install"
workspace_dir="${tmp_root}/workspace"
home_dir="${tmp_root}/home"
capture_port="${CLAUDE_INSIGHT_CAPTURE_PORT:-44180}"
mkdir -p "$install_root/bin" "$workspace_dir" "$home_dir"

cleanup() {
  HOME="$home_dir" CLAUDE_INSIGHT_HOME="$home_dir" "$binary" daemon stop >/dev/null 2>&1 || true
  rm -rf "$tmp_root"
}

tar -xzf "$asset_path" -C "$install_root/bin"
binary="${install_root}/bin/claude-insight"
chmod +x "$binary"
trap cleanup EXIT

help_output="$("$binary" --help)"
assert_contains "$help_output" "Local observability for Claude Code"

first_launch_output="$(
  HOME="$home_dir" \
  CLAUDE_INSIGHT_HOME="$home_dir" \
  "$binary"
)"
assert_contains "$first_launch_output" "First-run guided setup"
assert_contains "$first_launch_output" "v${expected_version}"
if [[ "$first_launch_output" == *"Usage: claude-insight [COMMAND]"* ]]; then
  echo "default launch regressed to clap help" >&2
  exit 1
fi

init_output="$(
  HOME="$home_dir" \
  CLAUDE_INSIGHT_HOME="$home_dir" \
  CLAUDE_INSIGHT_CAPTURE_PORT="$capture_port" \
  "$binary" init --global
)"
assert_contains "$init_output" "Initialized"
assert_contains "$init_output" "v${expected_version}"

session_id="release-smoke-$(date +%s)"
backlog_path="${home_dir}/.claude-insight/backlog.jsonl"
for fixture in SessionStart UserPromptSubmit PreToolUse PostToolUse Stop; do
  payload="$(rewrite_fixture "tests/fixtures/hooks/${fixture}.json" "$session_id" "$workspace_dir")"
  printf '%s' "$payload" | \
    HOME="$home_dir" \
    CLAUDE_INSIGHT_HOME="$home_dir" \
    "$binary" hook-forward --backlog-path "$backlog_path" --capture-port "$capture_port"
done

trace_output="$(
  HOME="$home_dir" \
  CLAUDE_INSIGHT_HOME="$home_dir" \
  "$binary" trace "$session_id"
)"
assert_contains "$trace_output" "Trace"
assert_contains "$trace_output" "SessionStart"

search_output="$(
  HOME="$home_dir" \
  CLAUDE_INSIGHT_HOME="$home_dir" \
  "$binary" search Bash
)"
assert_contains "$search_output" "Search"
assert_contains "$search_output" "$session_id"

post_init_launch_output="$(
  HOME="$home_dir" \
  CLAUDE_INSIGHT_HOME="$home_dir" \
  "$binary"
)"
assert_contains "$post_init_launch_output" "◉ Sessions"
if [[ "$post_init_launch_output" == *"Usage: claude-insight [COMMAND]"* ]]; then
  echo "default post-init launch regressed to clap help" >&2
  exit 1
fi

if [[ "${CLAUDE_INSIGHT_RUN_REAL_CLAUDE:-0}" == "1" ]]; then
  echo "real Claude release validation is environment-specific and should be recorded separately with local auth evidence." >&2
else
  echo "real Claude release validation skipped; scripted installed-binary smoke passed." >&2
fi
