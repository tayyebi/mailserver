#!/usr/bin/env bash
set -euo pipefail

ok() { echo "OK"; exit 0; }
fail() { echo "FAIL"; exit 1; }

# 1) HTTPS TLS handshake (DNS + outbound 443)
if echo | openssl s_client -connect cloudflare.com:443 -servername cloudflare.com -verify_return_error -brief >/dev/null 2>&1; then
  ok
fi

# 2) SMTP TLS handshake to a known server (DNS + outbound 465)
# Uses swaks to connect and quit after banner; avoids sending mail
if swaks --server smtp.gmail.com:465 --tls --quit-after CONNECT >/dev/null 2>&1; then
  ok
fi

fail