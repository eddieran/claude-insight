#!/usr/bin/env bash
set -euo pipefail

home_root="${CLAUDE_INSIGHT_HOME:-$HOME}"
state_dir="${home_root}/.claude-insight"
pid_file="${state_dir}/daemon.pid"
capture_port="${CLAUDE_INSIGHT_CAPTURE_PORT:-4180}"
health_url="http://127.0.0.1:${capture_port}/health"

mkdir -p "${state_dir}"

if [[ -f "${pid_file}" ]]; then
  pid="$(tr -d '[:space:]' < "${pid_file}" || true)"
  if [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null; then
    exit 0
  fi
  rm -f "${pid_file}"
fi

if command -v curl >/dev/null 2>&1 && curl -fsS "${health_url}" >/dev/null 2>&1; then
  exit 0
fi

if command -v claude-insight >/dev/null 2>&1; then
  nohup claude-insight serve >/dev/null 2>&1 &
fi
