#!/bin/sh
set -e

echo "[entrypoint] INFO: creating data directories"
mkdir -p /data/ssl /data/dkim /data/mail /data/db

# Ensure required users exist (safety net for pre-built images)
echo "[entrypoint] INFO: ensuring required system users exist"
id vmail >/dev/null 2>&1 || { echo "[entrypoint] INFO: creating vmail user"; addgroup -S vmail 2>/dev/null; adduser -S -D -H -G vmail -s /sbin/nologin vmail 2>/dev/null; }
id opendkim >/dev/null 2>&1 || { echo "[entrypoint] INFO: creating opendkim user"; addgroup -S opendkim 2>/dev/null; adduser -S -D -H -G opendkim -s /sbin/nologin opendkim 2>/dev/null; }

if [ ! -f /data/ssl/cert.pem ] || [ ! -f /usr/share/dovecot/dh.pem ]; then
    echo "[entrypoint] INFO: generating TLS certificates and DH parameters for hostname=${HOSTNAME:-mailserver}"
    /usr/local/bin/mailserver gencerts
else
    echo "[entrypoint] INFO: TLS certificates and DH parameters already exist, skipping generation"
fi

echo "[entrypoint] INFO: seeding database"
/usr/local/bin/mailserver seed

echo "[entrypoint] INFO: generating mail service configs"
/usr/local/bin/mailserver genconfig

echo "[entrypoint] INFO: setting directory ownership"
chown -R vmail:vmail /data/mail
chown -R opendkim:opendkim /data/dkim

echo "[entrypoint] INFO: starting services"
# Trap signals for clean container shutdown
trap 'trap - TERM; kill 0' SIGTERM SIGINT SIGQUIT

# Postfix and Dovecot log to stdout directly (via /dev/stdout)
# tee duplicates output to /var/log/mail.log for fail2ban monitoring
touch /var/log/mail.log

dovecot -F 2>&1 | tee -a /var/log/mail.log &
DOVECOT_PID=$!
opendkim -f &
OPENDKIM_PID=$!
/usr/local/bin/mailserver serve &
MAILSERVER_PID=$!
postfix start-fg 2>&1 | tee -a /var/log/mail.log &
POSTFIX_PID=$!

# Monitor all services — exit if any process dies
while kill -0 $DOVECOT_PID 2>/dev/null && \
      kill -0 $OPENDKIM_PID 2>/dev/null && \
      kill -0 $MAILSERVER_PID 2>/dev/null && \
      kill -0 $POSTFIX_PID 2>/dev/null; do
    sleep 5
done
echo "[entrypoint] ERROR: a service has exited, shutting down"
