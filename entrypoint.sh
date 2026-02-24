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
trap 'kill $(jobs -p) 2>/dev/null; wait; exit 0' SIGTERM SIGINT SIGQUIT

# Tail mail log to stdout for Docker log visibility
# (Postfix and Dovecot write directly to /var/log/mail.log, no syslog needed)
touch /var/log/mail.log
tail -F /var/log/mail.log &

dovecot -F &
opendkim -f &
/usr/local/bin/mailserver serve &
postfix start-fg &

# Wait for any child process to exit
wait
