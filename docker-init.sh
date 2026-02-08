#!/bin/bash
set -e

echo "[init] Mail server initialization..."

# Create SSL directory
mkdir -p /data/ssl

# Generate self-signed certificates if they don't exist
if [ ! -f /data/ssl/cert.pem ] || [ ! -f /data/ssl/key.pem ]; then
    echo "[init] Generating self-signed SSL certificates..."
    
    # Get the mail host from environment or use localhost
    CN="${MAIL_HOST:-localhost}"
    
    openssl req -x509 -nodes -newkey rsa:2048 -sha256 \
        -subj "/CN=${CN}" \
        -addext "subjectAltName=DNS:${CN}" \
        -keyout /data/ssl/key.pem \
        -out /data/ssl/cert.pem \
        -days 365
    
    chmod 600 /data/ssl/key.pem
    chmod 644 /data/ssl/cert.pem
    
    echo "[init] ✓ SSL certificates generated"
else
    echo "[init] ✓ SSL certificates already exist"
fi

# Create other required directories
mkdir -p /data/logs
mkdir -p /data/mail
mkdir -p /data/admin
mkdir -p /data/mail-config/opendkim/keys
mkdir -p /data/mail-config/postfix
mkdir -p /data/mail-config/dovecot
mkdir -p /data/opendkim/keys
mkdir -p /data/dovecot
mkdir -p /data/pixel/socket

# Create log files if they don't exist
touch /data/logs/dovecot.log
touch /data/logs/postfix.log

echo "[init] ✓ All required directories created"
echo "[init] Initialization complete"
