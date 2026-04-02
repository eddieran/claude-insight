#!/usr/bin/env bash
set -euo pipefail

payload="$(cat)"
backlog_dir="${HOME}/.claude-insight"
backlog_file="${backlog_dir}/backlog.jsonl"

if ! printf '%s' "${payload}" | curl -s -X POST http://localhost:4180/hooks -d @- >/dev/null; then
  mkdir -p "${backlog_dir}"
  printf '%s\n' "${payload}" >> "${backlog_file}"
fi
