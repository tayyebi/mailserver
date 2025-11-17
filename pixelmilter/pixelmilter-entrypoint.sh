#!/bin/bash
set -euo pipefail

# create expected dirs (if they are bind-mounted these calls are harmless)
mkdir -p /data/pixel
mkdir -p /var/run/pixelmilter

# chown/chmod only if we are root. If the host mount prevents changing ownership,
# suppress errors to avoid noisy logs or fatal exit.
if [ "$(id -u)" = "0" ]; then
  chown -R pixel:pixel /data/pixel 2>/dev/null || true
  chown -R pixel:pixel /var/run/pixelmilter 2>/dev/null || true
  chmod 0755 /var/run/pixelmilter 2>/dev/null || true
fi

SOCKET=${PIXEL_MILTER_SOCKET:-/var/run/pixelmilter/pixel.sock}

# Ensure socket directory exists (again harmless if mount)
mkdir -p "$(dirname "$SOCKET")" 2>/dev/null || true
# Do not attempt to change ownership of bind-mounted directories if not root
if [ "$(id -u)" = "0" ]; then
  chown pixel:pixel "$(dirname "$SOCKET")" 2>/dev/null || true
  chmod 0755 "$(dirname "$SOCKET")" 2>/dev/null || true
fi

# If running as root, drop privileges to pixel user when running the binary.
# If not root (container already started as non-root), just exec directly.
if [ "$(id -u)" = "0" ]; then
  exec su pixel -s /bin/sh -c "/usr/local/bin/pixelmilter --socket \"$SOCKET\" \"\$@\"" -- "$@"
else
  exec /usr/local/bin/pixelmilter --socket "$SOCKET" "$@"
fi
