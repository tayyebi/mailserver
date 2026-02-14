#!/bin/sh
set -e

echo "[entrypoint] INFO: creating data directories"
mkdir -p /data/ssl /data/dkim /data/mail /data/db

# Ensure required users exist (safety net for pre-built images)
echo "[entrypoint] INFO: ensuring required system users exist"
id vmail >/dev/null 2>&1 || { echo "[entrypoint] INFO: creating vmail user"; addgroup -S vmail 2>/dev/null; adduser -S -D -H -G vmail -s /sbin/nologin vmail 2>/dev/null; }
id opendkim >/dev/null 2>&1 || { echo "[entrypoint] INFO: creating opendkim user"; addgroup -S opendkim 2>/dev/null; adduser -S -D -H -G opendkim -s /sbin/nologin opendkim 2>/dev/null; }

if [ ! -f /data/ssl/cert.pem ]; then
    echo "[entrypoint] INFO: generating self-signed TLS certificate for hostname=${HOSTNAME:-mailserver}"
    openssl req -new -newkey rsa:2048 -days 3650 -nodes -x509 \
        -subj "/CN=${HOSTNAME:-mailserver}" \
        -keyout /data/ssl/key.pem -out /data/ssl/cert.pem
    chmod 600 /data/ssl/key.pem
    echo "[entrypoint] INFO: TLS certificate generated successfully"
else
    echo "[entrypoint] INFO: TLS certificate already exists, skipping generation"
fi

# Generate DH parameters if they don't exist (for Dovecot TLS)
if [ ! -f /usr/share/dovecot/dh.pem ]; then
    echo "[entrypoint] INFO: generating Diffie-Hellman parameters (this may take a while)"
    mkdir -p /usr/share/dovecot
    openssl dhparam -out /usr/share/dovecot/dh.pem 2048
    echo "[entrypoint] INFO: DH parameters generated successfully"
else
    echo "[entrypoint] INFO: DH parameters already exist, skipping generation"
fi

echo "[entrypoint] INFO: seeding database"
/usr/local/bin/mailserver seed

echo "[entrypoint] INFO: generating mail service configs"
/usr/local/bin/mailserver genconfig

echo "[entrypoint] INFO: setting directory ownership"
chown -R vmail:vmail /data/mail
chown -R opendkim:opendkim /data/dkim

echo "[entrypoint] INFO: starting syslogd for Postfix/Dovecot logging to stdout"
syslogd -n -O- &

echo "[entrypoint] INFO: starting supervisord and all services"
exec supervisord -c /etc/supervisord.conf
