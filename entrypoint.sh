#!/bin/sh
set -e

mkdir -p /data/ssl /data/dkim /data/mail /data/db

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
