#!/usr/bin/env bash
set -euo pipefail

MAIL_DOMAIN="${MAIL_DOMAIN:-example.com}"
MAIL_HOST="${MAIL_HOST:-mail.${MAIL_DOMAIN}}"
TZ="${TZ:-UTC}"
SUBMISSION_USER="${SUBMISSION_USER:-}"
SUBMISSION_PASS="${SUBMISSION_PASS:-}"

echo "$TZ" > /etc/timezone || true

log() { echo "[mail] $*"; }

# Ensure dirs exist
mkdir -p /etc/postfix /etc/dovecot /etc/ssl/local \
         /var/spool/postfix /var/lib/dovecot /var/log/mail
chown -R postfix:postfix /var/spool/postfix
chown -R dovecot:dovecot /var/lib/dovecot
chown -R syslog:adm /var/log/mail

# Generate self-signed cert if missing (you can replace with a real cert anytime)
if [ ! -s /etc/ssl/local/mailserver.key ] || [ ! -s /etc/ssl/local/mailserver.crt ]; then
  log "Generating self-signed TLS cert (replace with your real certs for production)"
  openssl req -x509 -newkey rsa:4096 -sha256 -days 825 -nodes \
    -keyout /etc/ssl/local/mailserver.key \
    -out /etc/ssl/local/mailserver.crt \
    -subj "/CN=${MAIL_HOST}" \
    -addext "subjectAltName=DNS:${MAIL_HOST},DNS:${MAIL_DOMAIN}"
  chmod 640 /etc/ssl/local/mailserver.key
fi

# Seed Postfix config if missing
if [ ! -s /etc/postfix/main.cf ]; then
  log "Seeding Postfix config"
  cp /templates/postfix/main.cf.tmpl /etc/postfix/main.cf
  cp /templates/postfix/master.cf.tmpl /etc/postfix/master.cf
  postconf -e "myhostname=${MAIL_HOST}"
  postconf -e "mydomain=${MAIL_DOMAIN}"
  postconf -e "smtpd_tls_cert_file=/etc/ssl/local/mailserver.crt"
  postconf -e "smtpd_tls_key_file=/etc/ssl/local/mailserver.key"
fi

# Seed Dovecot config if missing
if [ ! -s /etc/dovecot/dovecot.conf ]; then
  log "Seeding Dovecot config"
  cp -r /templates/dovecot/* /etc/dovecot/
fi

# Ensure dovecot passwd exists
if [ ! -s /etc/dovecot/passwd ]; then
  touch /etc/dovecot/passwd
  chown dovecot:dovecot /etc/dovecot/passwd
  chmod 640 /etc/dovecot/passwd
fi

# Optionally add initial submission user
if [ -n "$SUBMISSION_USER" ] && [ -n "$SUBMISSION_PASS" ]; then
  if ! grep -q "^${SUBMISSION_USER}:" /etc/dovecot/passwd; then
    log "Adding initial submission user: ${SUBMISSION_USER}"
    HASH=$(doveadm pw -s SHA512-CRYPT -p "$SUBMISSION_PASS")
    echo "${SUBMISSION_USER}:${HASH}" >> /etc/dovecot/passwd
  fi
fi

# Rsyslog for mail logs to /var/log/mail/mail.log
if ! grep -q "mail.*" /etc/rsyslog.conf; then
  cat >> /etc/rsyslog.conf <<'EOF'
$ModLoad imuxsock
$ModLoad imklog
mail.* -/var/log/mail/mail.log
& stop
EOF
fi

service rsyslog start

# Start services
log "Starting Dovecot (SASL only)"
/usr/sbin/dovecot

log "Starting Postfix"
service postfix start

# Tail logs to keep container in foreground
touch /var/log/mail/mail.log
tail -F /var/log/mail/mail.log /var/log/syslog
