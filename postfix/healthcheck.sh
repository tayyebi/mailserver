#!/usr/bin/env bash
set -euo pipefail

ok()   { echo "OK"; exit 0; }
fail() { echo "FAIL"; exit 1; }

# DNS resolution check — will fail if domain can't be resolved
if getent hosts gmail.com >/dev/null 2>&1; then
  ok
fi

# Optional: SMTP connectivity check with swaks
if swaks --server smtp.gmail.com:465 --tls --quit-after CONNECT >/dev/null 2>&1; then
  ok
fi

fail