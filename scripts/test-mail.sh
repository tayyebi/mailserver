#!/usr/bin/env bash
# ~/d/mailserver/scripts/test-mail.sh
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${DIR}/.env"
TMPDIR="$(mktemp -d)"
cleanup(){ rm -rf "$TMPDIR"; }
trap cleanup EXIT

bold(){ printf '\033[1m%s\033[0m\n' "$*"; }
log(){ printf '[test] %s\n' "$*"; }
ok(){ printf '[ OK ] %s\n' "$*"; }
fail(){ printf '[FAIL] %s\n' "$*" >&2; exit 1; }

# Defaults (override via .env or CLI)
MAIL_DOMAIN="${MAIL_DOMAIN:-}"
MAIL_HOST="${MAIL_HOST:-}"
SUBMISSION_USER="${SUBMISSION_USER:-}"
SUBMISSION_PASS="${SUBMISSION_PASS:-}"
TO_ADDR=""
VERBOSE=0
SEND=0

usage(){
  cat <<EOF
Usage: $(basename "$0") [-u user] [-p pass] [-t to@example.com] [-s] [-v]
  -s   send a test message via port 587 (requires -u/-p and -t)
  -u   submission username (overrides .env)
  -p   submission password (overrides .env)
  -t   recipient email for test message
  -v   verbose
EOF
}

# Load .env if present
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC2046
  export $(grep -E '^[A-Z0-9_]+=' "$ENV_FILE" | sed 's/#.*//' | xargs -I{} echo {})
fi

while getopts ":u:p:t:sv" opt; do
  case "$opt" in
    u) SUBMISSION_USER="$OPTARG" ;;
    p) SUBMISSION_PASS="$OPTARG" ;;
    t) TO_ADDR="$OPTARG" ;;
    s) SEND=1 ;;
    v) VERBOSE=1 ;;
    *) usage; exit 1 ;;
  esac
done

[[ -n "${MAIL_HOST:-}" ]] || fail "MAIL_HOST is not set (set in .env)"
[[ -n "${MAIL_DOMAIN:-}" ]] || fail "MAIL_DOMAIN is not set (set in .env)"

bold "1) Containers up"
docker ps --format '{{.Names}}' | grep -qx "mail" || fail "mail container not running"
docker ps --format '{{.Names}}' | grep -qx "opendkim" || fail "opendkim container not running"
ok "mail and opendkim containers are running"

bold "2) Ports exposed (localhost)"
for p in 25 587; do
  if timeout 3 bash -c ">/dev/tcp/127.0.0.1/${p}" 2>/dev/null; then
    ok "Port ${p} open"
  else
    fail "Port ${p} not reachable on localhost"
  fi
done

bold "3) STARTTLS on 587"
TLS_OUT="${TMPDIR}/tls.txt"
if openssl s_client -starttls smtp -crlf -connect 127.0.0.1:587 -servername "$MAIL_HOST" </dev/null >"$TLS_OUT" 2>&1; then
  grep -q "250-" "$TLS_OUT" || fail "No 250 capability lines after EHLO"
  grep -Eiq "subject=.*CN=?${MAIL_HOST}" "$TLS_OUT" || log "Note: certificate CN/SAN may not match ${MAIL_HOST}"
  ok "STARTTLS handshake successful"
  [[ $VERBOSE -eq 1 ]] && sed -n '1,30p' "$TLS_OUT"
else
  fail "STARTTLS handshake failed (see $TLS_OUT)"
fi

bold "4) SASL AUTH (PLAIN) on 587"
if [[ -n "${SUBMISSION_USER:-}" && -n "${SUBMISSION_PASS:-}" ]]; then
  AUTH_OUT="${TMPDIR}/auth.txt"
  AUTH=$(printf '\0%s\0%s' "$SUBMISSION_USER" "$SUBMISSION_PASS" | base64 -w0)
  {
    echo "EHLO test.${MAIL_DOMAIN}"
    echo "STARTTLS"
    echo ""
    echo "EHLO test.${MAIL_DOMAIN}"
    echo "AUTH PLAIN ${AUTH}"
    echo "QUIT"
  } | openssl s_client -quiet -starttls smtp -crlf -connect 127.0.0.1:587 -servername "$MAIL_HOST" >"$AUTH_OUT" 2>&1 || true
  grep -q "^235 " "$AUTH_OUT" || { sed -n '1,200p' "$AUTH_OUT" >&2; fail "SASL authentication failed"; }
  ok "SASL AUTH succeeded for ${SUBMISSION_USER}"
else
  log "Skipping AUTH: SUBMISSION_USER/SUBMISSION_PASS not set (use -u and -p)"
fi

if [[ $SEND -eq 1 ]]; then
  bold "5) Send test message via 587 (and check DKIM logs)"
  [[ -n "${SUBMISSION_USER:-}" && -n "${SUBMISSION_PASS:-}" ]] || fail "Need -u and -p to send"
  [[ -n "${TO_ADDR:-}" ]] || fail "Need -t recipient to send"

  MSG_ID="test-$(date +%s)-$$@${MAIL_HOST}"
  NOW="$(date -Ru)"
  FROM="${SUBMISSION_USER}"
  AUTH=$(printf '\0%s\0%s' "$SUBMISSION_USER" "$SUBMISSION_PASS" | base64 -w0)
  SEND_OUT="${TMPDIR}/send.txt"

  {
    echo "EHLO test.${MAIL_DOMAIN}"
    echo "STARTTLS"
    echo ""
    echo "EHLO test.${MAIL_DOMAIN}"
    echo "AUTH PLAIN ${AUTH}"
    echo "MAIL FROM:<${FROM}>"
    echo "RCPT TO:<${TO_ADDR}>"
    echo "DATA"
    echo "From: ${FROM}"
    echo "To: ${TO_ADDR}"
    echo "Date: ${NOW}"
    echo "Message-ID: <${MSG_ID}>"
    echo "Subject: Mailserver E2E test"
    echo
    echo "This is a test from ${MAIL_HOST} at ${NOW}."
    echo "."
    echo "QUIT"
  } | openssl s_client -quiet -starttls smtp -crlf -connect 127.0.0.1:587 -servername "$MAIL_HOST" >"$SEND_OUT" 2>&1 || true

  grep -E "250 2\.0\.0 Ok: queued|250 2\.6\.0" "$SEND_OUT" >/dev/null || { sed -n '1,200p' "$SEND_OUT"; fail "Message not accepted/queued"; }
  ok "Message accepted by Postfix (queued)"

  # Give opendkim a moment to sign/log
  sleep 3
  if docker logs --since 2m opendkim