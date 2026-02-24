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
trap 'kill $DOVECOT_PID $OPENDKIM_PID $MAILSERVER_PID $POSTFIX_PID $TAIL_PID 2>/dev/null; wait; exit 0' SIGTERM SIGINT SIGQUIT

# Tail mail log to stdout for Docker log visibility
# (Postfix and Dovecot write directly to /var/log/mail.log, no syslog needed)
touch /var/log/mail.log
tail -F /var/log/mail.log &
TAIL_PID=$!

dovecot -F &
DOVECOT_PID=$!
opendkim -f &
OPENDKIM_PID=$!
/usr/local/bin/mailserver serve &
MAILSERVER_PID=$!
postfix start-fg &
POSTFIX_PID=$!

# Monitor all services — exit if any critical process dies
while kill -0 $DOVECOT_PID 2>/dev/null && \
      kill -0 $OPENDKIM_PID 2>/dev/null && \
      kill -0 $MAILSERVER_PID 2>/dev/null && \
      kill -0 $POSTFIX_PID 2>/dev/null; do
    sleep 5
done
echo "[entrypoint] ERROR: a service has exited, shutting down"
