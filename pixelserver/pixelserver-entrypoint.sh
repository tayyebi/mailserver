#!/bin/bash
set -euo pipefail

# Create expected dirs (if they are bind-mounted these calls are harmless)
mkdir -p /data/pixel

# chown/chmod only if we are root. If the host mount prevents changing ownership,
# suppress errors to avoid noisy logs or fatal exit.
if [ "$(id -u)" = "0" ]; then
  chown -R pixelserver:pixelserver /data/pixel 2>/dev/null || true
  chmod 0755 /data/pixel 2>/dev/null || true
fi

# If running as root, drop privileges to pixelserver user when running the binary.
# If not root (container already started as non-root), just exec directly.
if [ "$(id -u)" = "0" ]; then
  exec su pixelserver -s /bin/sh -c "/usr/local/bin/pixelserver \"\$@\"" -- "$@"
else
  exec /usr/local/bin/pixelserver "$@"
fi
