#!/usr/bin/env bash
set -euo pipefail

ok()   { echo "OK"; exit 0; }
fail() { echo "FAIL"; exit 1; }

# Check if postfix master process is running
if pgrep -x master >/dev/null; then
  ok
fi

fail