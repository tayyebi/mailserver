#!/bin/bash
set -euo pipefail

# create expected dirs (if they are bind-mounted these calls are harmless)
mkdir -p /data/pixel
mkdir -p /var/run/pixelmilter

# chown/chmod only if we are root. If the host mount prevents changing ownership,
# suppress errors to avoid noisy logs or fatal exit.
if [ "$(id -u)" = "0" ]; then
  chown -R nobody:nogroup /data/pixel 2>/dev/null || true
  chown -R nobody:nogroup /var/run/pixelmilter 2>/dev/null || true
  chmod 0777 /var/run/pixelmilter 2>/dev/null || true
fi

SOCKET=${PIXEL_MILTER_SOCKET:-/var/run/pixelmilter/pixel.sock}

# Ensure socket directory exists (again harmless if mount)
mkdir -p "$(dirname "$SOCKET")" 2>/dev/null || true
# Do not attempt to change ownership of bind-mounted directories if not root
if [ "$(id -u)" = "0" ]; then
  chmod 0777 "$(dirname "$SOCKET")" 2>/dev/null || true
fi

# If running as root, drop privileges to nobody when running the perl script.
# If not root (container already started as non-root), just exec directly.
if [ "$(id -u)" = "0" ]; then
  # Prefer gosu/su-exec if present; fallback to su -s /bin/sh -c
  if command -v gosu >/dev/null 2>&1; then
    exec gosu nobody:nogroup /usr/local/bin/pixelmilter.pl --socket "$SOCKET"
  elif command -v su-exec >/dev/null 2>&1; then
    exec su-exec nobody:nogroup /usr/local/bin/pixelmilter.pl --socket "$SOCKET"
  else
    # Fallback: use su (may require passwd entry for nobody — unlikely to fail)
    exec su nobody -s /bin/sh -c "/usr/local/bin/pixelmilter.pl --socket \"$SOCKET\""
  fi
else
  exec /usr/local/bin/pixelmilter.pl --socket "$SOCKET"
fi
