#!/usr/bin/env bash
set -euo pipefail
DOMAIN="${1:-}"
SEL="${2:-default}"

CONF_DIR="/etc/opendkim"
KEYS_DIR="/etc/opendkim/keys/${DOMAIN}"
[ -n "$DOMAIN" ] || { echo "Usage: add-domain.sh DOMAIN [selector]"; exit 1; }

mkdir -p "$KEYS_DIR"
chown -R opendkim:opendkim "$CONF_DIR" "$KEYS_DIR"
chmod 750 "$KEYS_DIR"

if [ ! -f "${KEYS_DIR}/${SEL}.private" ]; then
  opendkim-genkey -D "$KEYS_DIR" -d "$DOMAIN" -s "$SEL"
  chown opendkim:opendkim "${KEYS_DIR}/${SEL}.private" "${KEYS_DIR}/${SEL}.txt"
  chmod 600 "${KEYS_DIR}/${SEL}.private"
fi

grep -q "^${SEL}._domainkey.${DOMAIN}\b" "${CONF_DIR}/KeyTable" 2>/dev/null || \
  echo "${SEL}._domainkey.${DOMAIN} ${DOMAIN}:${SEL}:${KEYS_DIR}/${SEL}.private" >> "${CONF_DIR}/KeyTable"

grep -q "^\*@${DOMAIN}\b" "${CONF_DIR}/SigningTable" 2>/dev/null || \
  echo "*@${DOMAIN}       ${SEL}._domainkey.${DOMAIN}" >> "${CONF_DIR}/SigningTable"

grep -q "^${DOMAIN}\b" "${CONF_DIR}/TrustedHosts" 2>/dev/null || \
  echo "${DOMAIN}" >> "${CONF_DIR}/TrustedHosts"

echo "DKIM TXT record for ${SEL}._domainkey.${DOMAIN}:"
cat "${KEYS_DIR}/${SEL}.txt"
