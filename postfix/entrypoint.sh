#!/usr/bin/env bash
set -euo pipefail

# expand templates and validate
if [ ! -s /etc/postfix/main.cf ]; then
  log "Rendering Postfix config"
  render_template /templates/postfix/main.cf.tmpl /etc/postfix/main.cf
  render_template /templates/postfix/master.cf.tmpl /etc/postfix/master.cf
fi
for f in virtual_aliases virtual_domains vmailbox; do
  [ -s "/etc/postfix/$f" ] || render_template "/templates/postfix/${f}.tmpl" "/etc/postfix/$f"
done
for f in virtual_aliases virtual_domains vmailbox; do
  grep '\${' "/etc/postfix/$f" && {
    log "Unresolved variable in $f"
    cat "/etc/postfix/$f"
    exit 1
  }
done
# ensure trailing empty line and remove key before adding it
sed -i -e '$a\' /etc/postfix/main.cf
for key in smtpd_sasl_type smtpd_sasl_path virtual_transport; do
  postconf -X "$key" || true
done


MAIL_DOMAIN="${MAIL_DOMAIN:-example.com}"
MAIL_HOST="${MAIL_HOST:-mail.${MAIL_DOMAIN}}"
TZ="${TZ:-UTC}"
SUBMISSION_USER="${SUBMISSION_USER:-}"
SUBMISSION_PASS="${SUBMISSION_PASS:-}"
DOVECOT_AUTH_HOST="${DOVECOT_AUTH_HOST:-dovecot}"
DOVECOT_AUTH_PORT="${DOVECOT_AUTH_PORT:-12345}"
DOVECOT_LMTP_HOST="${DOVECOT_LMTP_HOST:-dovecot}"
DOVECOT_LMTP_PORT="${DOVECOT_LMTP_PORT:-24}"

echo "$TZ" > /etc/timezone || true

log() { echo "[postfix] $*"; }

mkdir -p /etc/postfix /var/spool/postfix /var/lib/postfix /var/log/mail
chown -R postfix:postfix /var/spool/postfix /var/lib/postfix

# Seed configs if missing
if [ ! -s /etc/postfix/main.cf ]; then
  log "Seeding Postfix config"
  cp /templates/postfix/main.cf.tmpl /etc/postfix/main.cf
  cp /templates/postfix/master.cf.tmpl /etc/postfix/master.cf
fi

# Seed maps if missing
[ -s /etc/postfix/virtual_aliases ] || cp /templates/postfix/virtual_aliases.tmpl /etc/postfix/virtual_aliases
[ -s /etc/postfix/virtual_domains ] || cp /templates/postfix/virtual_domains.tmpl /etc/postfix/virtual_domains
[ -s /etc/postfix/vmailbox ] || cp /templates/postfix/vmailbox.tmpl /etc/postfix/vmailbox

# Ensure TLS points to shared volume
postconf -e "myhostname=${MAIL_HOST}"
postconf -e "mydomain=${MAIL_DOMAIN}"
postconf -e "smtpd_tls_cert_file=/etc/ssl/private/cert.pem"
postconf -e "smtpd_tls_key_file=/etc/ssl/private/key.pem"

# SASL via Dovecot over TCP
postconf -e "smtpd_sasl_type=dovecot"
postconf -e "smtpd_sasl_path=inet:${DOVECOT_AUTH_HOST}:${DOVECOT_AUTH_PORT}"

# LMTP over TCP to Dovecot
postconf -e "virtual_transport=lmtp:inet:${DOVECOT_LMTP_HOST}:${DOVECOT_LMTP_PORT}"

# Postmap
for f in virtual_aliases virtual_domains vmailbox; do
  postmap "/etc/postfix/$f" || true
done

log "Starting Postfix (foreground)"
exec /usr/sbin/postfix start-fg