#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMUX_CMD_C="$ROOT_DIR/tmux/cmd.c"
OUT="$ROOT_DIR/dmux/docs/tmux-command-coverage.md"

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required" >&2
  exit 1
fi

if [[ ! -f "$TMUX_CMD_C" ]]; then
  echo "error: missing $TMUX_CMD_C" >&2
  exit 1
fi

{
  echo "# tmux Command Coverage Tracker"
  echo
  echo 'Generated from: `tmux/cmd.c`'
  echo
  echo "Status legend:"
  echo "- [ ] not started"
  echo "- [x] implemented in dmux"
  echo
  rg -o '&cmd_[a-z0-9_]+_entry' "$TMUX_CMD_C" \
    | sed 's/&cmd_//; s/_entry//' \
    | sort -u \
    | sed 's/.*/- [ ] &/'
} > "$OUT"

echo "wrote $OUT"
