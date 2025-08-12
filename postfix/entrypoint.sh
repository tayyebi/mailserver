#!/usr/bin/env bash
set -euo pipefail

log() { echo "[postfix] $*"; }

# 1. Render templates if missing
if [ ! -s /etc/postfix/main.cf ]; then
  log "Rendering Postfix config"
  render_template /templates/postfix/main.cf.tmpl /etc/postfix/main.cf
  render_template /templates/postfix/master.cf.tmpl /etc/postfix/master.cf

  # Dump the result for debug
  log "----- /etc/postfix/main.cf -----"
  cat /etc/postfix/main.cf
  log "--------------------------------"

  # Sanity-check: no merged directives (e.g. missing newline between two keys)
  if grep -q '=[^[:space:]].*=[^[:space:]]' /etc/postfix/main.cf; then
    log "Malformed directive detected in main.cf"
    exit 1
  fi
fi

# 2. Render any map files if missing
for f in virtual_aliases virtual_domains vmailbox; do
  [ -s "/etc/postfix/$f" ] || render_template "/templates/postfix/${f}.tmpl" "/etc/postfix/$f"
done

# 3. Ensure no unresolved variables in the maps
for f in virtual_aliases virtual_domains vmailbox; do
  if grep '\${' "/etc/postfix/$f"; then
    log "Unresolved variable in $f"
    cat "/etc/postfix/$f"
    exit 1
  fi
done

# 4. Guarantee trailing newline in main.cf
sed -i -e '$a\' /etc/postfix/main.cf

# 5. Remove old keys to avoid appending invalid values
for key in smtpd_sasl_type smtpd_sasl_path virtual_transport; do
  postconf -X "$key" || true
done

# 6. Load environment defaults
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
ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime

# 7. Ensure file system layout & permissions
mkdir -p /etc/postfix /var/spool/postfix /var/lib/postfix /var/log/mail
chown -R postfix:postfix /var/spool/postfix /var/lib/postfix

# 8. Seed configs if user-volume is empty
if [ ! -s /etc/postfix/main.cf ]; then
  log "Seeding Postfix config from templates"
  cp /templates/postfix/main.cf.tmpl /etc/postfix/main.cf
  cp /templates/postfix/master.cf.tmpl /etc/postfix/master.cf
fi
[ -s /etc/postfix/virtual_aliases ] || cp /templates/postfix/virtual_aliases.tmpl /etc/postfix/virtual_aliases
[ -s /etc/postfix/virtual_domains ]  || cp /templates/postfix/virtual_domains.tmpl  /etc/postfix/virtual_domains
[ -s /etc/postfix/vmailbox ]         || cp /templates/postfix/vmailbox.tmpl         /etc/postfix/vmailbox

# 9. Apply dynamic Postfix settings
postconf -e "myhostname=${MAIL_HOST}"
postconf -e "mydomain=${MAIL_DOMAIN}"
postconf -e "smtpd_tls_cert_file=/etc/ssl/private/cert.pem"
postconf -e "smtpd_tls_key_file=/etc/ssl/private/key.pem"

postconf -e "smtpd_sasl_type=dovecot"
postconf -e "smtpd_sasl_path=inet:${DOVECOT_AUTH_HOST}:${DOVECOT_AUTH_PORT}"

postconf -e "virtual_transport=lmtp:inet:${DOVECOT_LMTP_HOST}:${DOVECOT_LMTP_PORT}"

# 10. Compile lookup tables
for f in virtual_aliases virtual_domains vmailbox; do
  postmap "/etc/postfix/$f" || true
done

# 11. Lint the entire Postfix configuration
log "Running postfix check"
if ! postfix check; then
  log "Postfix config validation failed"
  exit 1
fi

# 12. Launch Postfix in foreground
log "Starting Postfix (foreground)"
exec /usr/sbin/postfix start-fg