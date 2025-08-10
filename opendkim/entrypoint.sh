#!/usr/bin/env bash
set -euo pipefail

MAIL_DOMAIN="${MAIL_DOMAIN:-example.com}"
DKIM_SELECTOR="${DKIM_SELECTOR:-mail}"
KEYS_DIR="/etc/opendkim/keys/${MAIL_DOMAIN}"

log() { echo "[opendkim] $*"; }

mkdir -p "$KEYS_DIR"
if [ ! -s "${KEYS_DIR}/${DKIM_SELECTOR}.private" ]; then
  log "Generating DKIM key for ${MAIL_DOMAIN} selector ${DKIM_SELECTOR}"
  opendkim-genkey -D "$KEYS_DIR" -d "$MAIL_DOMAIN" -s "$DKIM_SELECTOR"
  chown -R opendkim:opendkim "$KEYS_DIR"
  chmod 600 "${KEYS_DIR}/${DKIM_SELECTOR}.private"
fi

# Build config files if missing
[ -s /etc/opendkim/TrustedHosts ] || cat > /etc/opendkim/TrustedHosts <<EOF
127.0.0.1
localhost
mail
${MAIL_DOMAIN}
EOF

[ -s /etc/opendkim/KeyTable ] || cat > /etc/opendkim/KeyTable <<EOF
${DKIM_SELECTOR}._domainkey.${MAIL_DOMAIN} ${MAIL_DOMAIN}:${DKIM_SELECTOR}:${KEYS_DIR}/${DKIM_SELECTOR}.private
EOF

[ -s /etc/opendkim/SigningTable ] || cat > /etc/opendkim/SigningTable <<EOF
*@${MAIL_DOMAIN} ${DKIM_SELECTOR}._domainkey.${MAIL_DOMAIN}
EOF

[ -s /etc/opendkim/opendkim.conf ] || cat > /etc/opendkim/opendkim.conf <<'EOF'
Syslog                  yes
UMask                   002
Mode                    s
UserID                  opendkim:opendkim
Socket                  inet:8891@0.0.0.0
PidFile                 /var/run/opendkim/opendkim.pid
Selector                default
Canonicalization        relaxed/simple
MinimumKeyBits          1024
KeyTable                /etc/opendkim/KeyTable
SigningTable            /etc/opendkim/SigningTable
ExternalIgnoreList      /etc/opendkim/TrustedHosts
InternalHosts           /etc/opendkim/TrustedHosts
OversignHeaders         From
AutoRestart             Yes
EOF

# Show DNS TXT to help publish
if [ -f "${KEYS_DIR}/${DKIM_SELECTOR}.txt" ]; then
  log "DKIM DNS record (add to DNS as TXT for ${DKIM_SELECTOR}._domainkey.${MAIL_DOMAIN}):"
  sed 's/" "/"/' "${KEYS_DIR}/${DKIM_SELECTOR}.txt" | sed 's/( "v=/( "v=/'
fi

exec /usr/sbin/opendkim -f -x /etc/opendkim/opendkim.conf
