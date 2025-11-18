#!/bin/bash
set -e

SSL_DIR="ssl"
mkdir -p "$SSL_DIR"

DAYS=3650
COUNTRY="IR"
ORG="Gordarg"
CN="mail.gordarg.com"

echo "🔐 Generating self-signed certificates in $SSL_DIR..."

# Dovecot & Postfix TLS cert
openssl req -new -x509 -days $DAYS -nodes \
  -out "$SSL_DIR/cert.pem" \
  -keyout "$SSL_DIR/key.pem" \
  -subj "/C=$COUNTRY/O=$ORG/CN=$CN"

# OpenDKIM keys
DKIM_DOMAIN="gordarg.com"
DKIM_SELECTOR="mail"
DKIM_DIR="$SSL_DIR/opendkim"
mkdir -p "$DKIM_DIR/keys/$DKIM_DOMAIN"

# Create trusted certs for OpenDKIM
cat "$SSL_DIR/cert.pem" > "$DKIM_DIR/trusted-hosts"

echo "✅ Certificates generated:"
ls -l "$SSL_DIR"