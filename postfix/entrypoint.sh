#!/usr/bin/env bash
set -euo pipefail

log() { echo "[postfix] $*"; }
render_template() {
  envsubst < "$1" > "$2"
}

# Render templates if missing
if [ ! -s /etc/postfix/main.cf ]; then
  log "Rendering Postfix config"
  render_template /templates/main.cf.tmpl /etc/postfix/main.cf
  render_template /templates/master.cf.tmpl /etc/postfix/master.cf
  render_template /templates/virtual_aliases.tmpl /etc/postfix/virtual_aliases
  render_template /templates/virtual_domains.tmpl /etc/postfix/virtual_domains
  render_template /templates/vmailbox.tmpl /etc/postfix/vmailbox

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

# Load environment defaults
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

# Ensure file system layout & permissions
mkdir -p /etc/postfix /var/spool/postfix /var/lib/postfix /var/log/mail
chown -R postfix:postfix /var/spool/postfix /var/lib/postfix
chmod -R 755 /var/spool/postfix

# Seed configs if user-volume is empty
if [ ! -s /etc/postfix/main.cf ]; then
  log "Seeding Postfix config from templates"
  cp /templates/postfix/main.cf.tmpl /etc/postfix/main.cf
  cp /templates/postfix/master.cf.tmpl /etc/postfix/master.cf
fi
[ -s /etc/postfix/virtual_aliases ] || cp /templates/postfix/virtual_aliases.tmpl /etc/postfix/virtual_aliases
[ -s /etc/postfix/virtual_domains ]  || cp /templates/postfix/virtual_domains.tmpl  /etc/postfix/virtual_domains
[ -s /etc/postfix/vmailbox ]         || cp /templates/postfix/vmailbox.tmpl         /etc/postfix/vmailbox

# Compile lookup tables
for f in virtual_aliases virtual_domains vmailbox; do
  postmap "/etc/postfix/$f" || true
done

# Lint the entire Postfix configuration
log "Running postfix check"
if ! postfix check; then
  log "Postfix config validation failed"
  exit 1
fi

# Launch Postfix in foreground
log "Starting Postfix"
exec /usr/sbin/postfix -vvv start-fg