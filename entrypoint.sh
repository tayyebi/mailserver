#!/bin/sh
set -e

mkdir -p /data/ssl /data/dkim /data/mail /data/db

# Ensure required users exist (safety net for pre-built images)
id vmail >/dev/null 2>&1 || { addgroup -S vmail 2>/dev/null; adduser -S -D -H -G vmail -s /sbin/nologin vmail 2>/dev/null; }
id opendkim >/dev/null 2>&1 || { addgroup -S opendkim 2>/dev/null; adduser -S -D -H -G opendkim -s /sbin/nologin opendkim 2>/dev/null; }

if [ ! -f /data/ssl/cert.pem ]; then
    echo "Generating self-signed TLS certificate..."
    openssl req -new -newkey rsa:2048 -days 3650 -nodes -x509 \
        -subj "/CN=${HOSTNAME:-mailserver}" \
        -keyout /data/ssl/key.pem -out /data/ssl/cert.pem
    chmod 600 /data/ssl/key.pem
fi

echo "Seeding database..."
/usr/local/bin/mailserver seed

echo "Generating mail service configs..."
/usr/local/bin/mailserver genconfig

chown -R vmail:vmail /data/mail
chown -R opendkim:opendkim /data/dkim

echo "Starting services..."
exec supervisord -c /etc/supervisord.conf
