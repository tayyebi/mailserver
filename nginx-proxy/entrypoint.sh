#!/bin/sh
set -e

echo "[nginx-proxy] Initializing..."

# Require basic auth credentials for /admin
if [ -z "${ADMIN_BASIC_AUTH_USER}" ] || [ -z "${ADMIN_BASIC_AUTH_PASSWORD}" ]; then
  echo "[nginx-proxy] ERROR: ADMIN_BASIC_AUTH_USER and ADMIN_BASIC_AUTH_PASSWORD must be set in the environment for admin basic auth."
  echo "[nginx-proxy]       Example: ADMIN_BASIC_AUTH_USER=admin ADMIN_BASIC_AUTH_PASSWORD=strongpassword"
  exit 1
fi

echo "[nginx-proxy] Creating htpasswd file for admin basic auth user '${ADMIN_BASIC_AUTH_USER}'..."
htpasswd -bc /etc/nginx/.htpasswd "${ADMIN_BASIC_AUTH_USER}" "${ADMIN_BASIC_AUTH_PASSWORD}"

echo "[nginx-proxy] Starting nginx..."
exec nginx -g 'daemon off;'
