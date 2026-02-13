#!/bin/sh
set -e

# --- Self-signed SSL (generate once, reuse on restart) ---
if [ ! -f /etc/nginx/ssl/nginx-selfsigned.crt ] || [ ! -f /etc/nginx/ssl/nginx-selfsigned.key ]; then
  echo "[nginx-proxy] Generating self-signed certificate..."
  openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
    -keyout /etc/nginx/ssl/nginx-selfsigned.key \
    -out /etc/nginx/ssl/nginx-selfsigned.crt \
    -subj "/CN=${MAIL_HOST:-localhost}" 2>/dev/null
else
  echo "[nginx-proxy] Using existing certificate found in /etc/nginx/ssl/"
fi

# --- Basic Auth ---
if [ -z "${ADMIN_BASIC_AUTH_USER}" ] || [ -z "${ADMIN_BASIC_AUTH_PASSWORD}" ]; then
  echo "[nginx-proxy] ERROR: ADMIN_BASIC_AUTH_USER and ADMIN_BASIC_AUTH_PASSWORD must be set in the environment for admin basic auth."
  echo "[nginx-proxy]       Example: ADMIN_BASIC_AUTH_USER=admin ADMIN_BASIC_AUTH_PASSWORD=strongpassword"
  exit 1
fi
htpasswd -bc /etc/nginx/.htpasswd "${ADMIN_BASIC_AUTH_USER}" "${ADMIN_BASIC_AUTH_PASSWORD}" 2>/dev/null
echo "[nginx-proxy] Ready (auth user: ${ADMIN_BASIC_AUTH_USER})"

# Hand off to the official nginx entrypoint (envsubst, etc.)
exec /docker-entrypoint.sh nginx -g 'daemon off;'
